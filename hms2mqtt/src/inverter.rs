use crate::protos::hoymiles::RealData::{HMSStateResponse, RealDataResDTO};
use crc16::{State, MODBUS};
use log::{debug, error, info, warn};
use protobuf::Message;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

static INVERTER_PORT: u16 = 10081;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NetworkState {
    Unknown,
    Online,
    Offline,
}

pub struct Inverter<'a> {
    host: &'a str,
    port: u16,
    state: NetworkState,
    sequence: u16,
}

impl<'a> Inverter<'a> {
    pub fn new(host: &'a str) -> Self {
        Self::with_port(host, INVERTER_PORT)
    }

    pub fn with_port(host: &'a str, port: u16) -> Self {
        Self {
            host,
            port,
            state: NetworkState::Unknown,
            sequence: 0_u16,
        }
    }

    pub fn state(&self) -> NetworkState {
        self.state
    }

    fn set_state(&mut self, new_state: NetworkState) {
        if self.state != new_state {
            self.state = new_state;
            info!("Inverter is {new_state:?}");
        }
    }

    pub fn update_state(&mut self) -> Option<HMSStateResponse> {
        self.sequence = self.sequence.wrapping_add(1);

        let /*mut*/ request = RealDataResDTO::default();
        // let date = Local::now();
        // let time_string = date.format("%Y-%m-%d %H:%M:%S").to_string();
        // request.ymd_hms = time_string;
        // request.cp = 23 + sequence as i32;
        // request.offset = 0;
        // request.time = epoch();
        let header = b"\x48\x4d\xa3\x03";
        let request_as_bytes = request.write_to_bytes().expect("serialize to bytes");
        let crc16 = State::<MODBUS>::calculate(&request_as_bytes);
        let len = request_as_bytes.len() as u16 + 10u16;

        // compose request message
        let mut message = Vec::new();
        message.extend_from_slice(header);
        message.extend_from_slice(&self.sequence.to_be_bytes());
        message.extend_from_slice(&crc16.to_be_bytes());
        message.extend_from_slice(&len.to_be_bytes());
        message.extend_from_slice(&request_as_bytes);

        let address = match (self.host, self.port).to_socket_addrs() {
            Ok(mut a) => a.next(),
            Err(e) => {
                error!("Unable to resolve domain: {e}");
                return None;
            }
        };
        if address.is_none() {
            error!("Unable to parse name");
            return None;
        }

        let stream = TcpStream::connect_timeout(&address.unwrap(), Duration::from_millis(500));
        if let Err(e) = stream {
            debug!("could not connect: {e}");
            self.set_state(NetworkState::Offline);
            return None;
        }

        let mut stream = stream.unwrap();
        if let Err(e) = stream.set_write_timeout(Some(Duration::new(5, 0))) {
            warn!("could not set write timeout: {e}");
        }
        if let Err(e) = stream.set_read_timeout(Some(Duration::new(5, 0))) {
            warn!("could not set read timeout: {e}");
        }
        if let Err(e) = stream.write_all(&message) {
            debug!(r#"{e}"#);
            self.set_state(NetworkState::Offline);
            return None;
        }

        let payload = match read_frame(&mut stream, self.sequence) {
            Ok(payload) => payload,
            Err(e) => {
                debug!("{e}");
                self.set_state(NetworkState::Offline);
                return None;
            }
        };
        let parsed = HMSStateResponse::parse_from_bytes(&payload);

        if let Err(e) = parsed {
            debug!("{e}");
            self.set_state(NetworkState::Offline);
            return None;
        }
        debug_assert!(parsed.is_ok());

        let response = parsed.unwrap();
        if is_stale(&response) {
            // The DTU answered, but its last read of the inverter(s) failed; the values
            // are a repeat of the previous reading and must not be published as fresh.
            warn!("DTU reports no inverter link (link == 0), skipping stale reading");
            return None;
        }
        self.set_state(NetworkState::Online);
        Some(response)
    }
}

const HEADER_LEN: usize = 10;
// replies grow with the number of inverters and ports; the app caps payloads at 4096 bytes
const MAX_FRAME_LEN: usize = HEADER_LEN + 4096;

/// Reads one HM frame ("HM" | cmd | seq | crc | len | payload) and returns its payload.
/// The length field covers header and payload, so the frame is read completely even if it
/// arrives in several TCP segments, and short or malformed replies become errors instead of panics.
/// The reply must be a DTU response (0xA2xx), echo `expected_seq` and carry a valid CRC-16/MODBUS.
pub fn read_frame<R: Read>(reader: &mut R, expected_seq: u16) -> Result<Vec<u8>, String> {
    let mut header = [0u8; HEADER_LEN];
    reader
        .read_exact(&mut header)
        .map_err(|e| format!("could not read frame header: {e}"))?;
    if &header[0..2] != b"HM" {
        return Err(format!("unexpected frame magic {:02x?}", &header[0..2]));
    }
    let cmd = u16::from_be_bytes([header[2], header[3]]);
    let seq = u16::from_be_bytes([header[4], header[5]]);
    let crc = u16::from_be_bytes([header[6], header[7]]);
    let len = u16::from_be_bytes([header[8], header[9]]) as usize;
    debug!("received frame cmd 0x{cmd:04x} seq {seq} len {len}");
    // responses are the request command - 0x100, e.g. 0xa303 is answered with 0xa203 or 0xa211
    if cmd >> 8 != 0xa2 {
        return Err(format!("unexpected response command 0x{cmd:04x}"));
    }
    if seq != expected_seq {
        return Err(format!(
            "unexpected sequence number {seq}, expected {expected_seq}"
        ));
    }
    if !(HEADER_LEN..=MAX_FRAME_LEN).contains(&len) {
        return Err(format!("invalid frame length {len}"));
    }
    let mut payload = vec![0u8; len - HEADER_LEN];
    reader
        .read_exact(&mut payload)
        .map_err(|e| format!("could not read frame payload: {e}"))?;
    let computed_crc = State::<MODBUS>::calculate(&payload);
    if computed_crc != crc {
        return Err(format!(
            "CRC mismatch: frame says 0x{crc:04x}, payload has 0x{computed_crc:04x}"
        ));
    }
    Ok(payload)
}

/// A reading is stale when no inverter reports a working link to the DTU.
pub fn is_stale(response: &HMSStateResponse) -> bool {
    response
        .inverter_state
        .iter()
        .all(|inverter| inverter.link == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protos::hoymiles::RealData::InverterState;

    fn response_with_links(links: &[i32]) -> HMSStateResponse {
        let mut response = HMSStateResponse::new();
        for &link in links {
            let mut inverter = InverterState::new();
            inverter.link = link;
            response.inverter_state.push(inverter);
        }
        response
    }

    #[test]
    fn linked_inverter_is_fresh() {
        assert!(!is_stale(&response_with_links(&[1])));
    }

    #[test]
    fn one_linked_inverter_is_enough() {
        assert!(!is_stale(&response_with_links(&[0, 1])));
    }

    #[test]
    fn unlinked_inverters_are_stale() {
        assert!(is_stale(&response_with_links(&[0])));
        assert!(is_stale(&response_with_links(&[0, 0])));
    }

    const SEQ: u16 = 1;

    /// Starts a fake DTU on localhost that answers `replies.len()` connections, one reply each.
    /// Each reply closure gets the request frame and returns the bytes to send back.
    /// Returns the port and a handle yielding the received request frames.
    #[allow(clippy::type_complexity)]
    fn fake_dtu(
        replies: Vec<Box<dyn Fn(&[u8]) -> Vec<u8> + Send>>,
    ) -> (u16, std::thread::JoinHandle<Vec<Vec<u8>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut conn, _) = listener.accept().unwrap();
                let mut header = [0u8; HEADER_LEN];
                conn.read_exact(&mut header).unwrap();
                let len = u16::from_be_bytes([header[8], header[9]]) as usize;
                let mut request = header.to_vec();
                request.resize(len, 0);
                conn.read_exact(&mut request[HEADER_LEN..]).unwrap();
                conn.write_all(&reply(&request)).unwrap();
                requests.push(request);
            }
            requests
        });
        (port, handle)
    }

    /// A DTU reply to `request` carrying `response`, with matching sequence number and valid CRC.
    fn reply_to(request: &[u8], response: &HMSStateResponse) -> Vec<u8> {
        let payload = response.write_to_bytes().unwrap();
        let mut f = b"HM\xa2\x11".to_vec();
        f.extend_from_slice(&request[4..6]);
        f.extend_from_slice(&State::<MODBUS>::calculate(&payload).to_be_bytes());
        f.extend_from_slice(&(HEADER_LEN as u16 + payload.len() as u16).to_be_bytes());
        f.extend_from_slice(&payload);
        f
    }

    #[test]
    fn update_state_returns_fresh_reading_and_sends_valid_request() {
        let mut fresh = response_with_links(&[1]);
        fresh.dtu_sn = "414312345678".to_string();
        let (port, dtu) = fake_dtu(vec![
            Box::new({
                let fresh = fresh.clone();
                move |req| reply_to(req, &fresh)
            }),
            Box::new(move |req| reply_to(req, &fresh)),
        ]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        assert_eq!(inverter.state(), NetworkState::Unknown);

        let reading = inverter.update_state().expect("fresh reading");
        assert_eq!(reading.dtu_sn, "414312345678");
        assert_eq!(inverter.state(), NetworkState::Online);
        assert!(inverter.update_state().is_some());

        let requests = dtu.join().unwrap();
        for (i, request) in requests.iter().enumerate() {
            assert_eq!(&request[0..4], b"HM\xa3\x03");
            // sequence numbers start at 1 and increase per request
            assert_eq!(u16::from_be_bytes([request[4], request[5]]), i as u16 + 1);
            let payload = &request[HEADER_LEN..];
            assert_eq!(
                u16::from_be_bytes([request[6], request[7]]),
                State::<MODBUS>::calculate(payload)
            );
            assert_eq!(
                u16::from_be_bytes([request[8], request[9]]) as usize,
                request.len()
            );
        }
    }

    #[test]
    fn update_state_skips_stale_reading() {
        let stale = response_with_links(&[0]);
        let (port, dtu) = fake_dtu(vec![Box::new(move |req| reply_to(req, &stale))]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        assert!(inverter.update_state().is_none());
        assert_ne!(inverter.state(), NetworkState::Online);
        dtu.join().unwrap();
    }

    #[test]
    fn update_state_handles_bad_replies_without_panicking() {
        let fresh = response_with_links(&[1]);
        let (port, dtu) = fake_dtu(vec![
            // closes the connection without answering
            Box::new(|_| Vec::new()),
            // too short to be a frame
            Box::new(|_| b"HM\xa2".to_vec()),
            // valid frame, wrong sequence number
            Box::new(move |req| {
                let mut f = reply_to(req, &fresh);
                f[5] ^= 0xff;
                f
            }),
            // valid header, payload is not protobuf
            Box::new(|req| {
                let payload = [0xffu8; 4];
                let mut f = b"HM\xa2\x11".to_vec();
                f.extend_from_slice(&req[4..6]);
                f.extend_from_slice(&State::<MODBUS>::calculate(&payload).to_be_bytes());
                f.extend_from_slice(&(HEADER_LEN as u16 + 4).to_be_bytes());
                f.extend_from_slice(&payload);
                f
            }),
        ]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        for _ in 0..4 {
            assert!(inverter.update_state().is_none());
            assert_eq!(inverter.state(), NetworkState::Offline);
        }
        dtu.join().unwrap();
    }

    #[test]
    fn update_state_marks_unreachable_inverter_offline() {
        // bind and drop a listener to get a local port nobody listens on
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        assert!(inverter.update_state().is_none());
        assert_eq!(inverter.state(), NetworkState::Offline);
    }

    #[test]
    fn update_state_rejects_unresolvable_host() {
        let mut inverter = Inverter::with_port("host.invalid", 10081);
        assert!(inverter.update_state().is_none());
    }

    /// A frame with the given command and length field and a correct CRC over `payload`.
    fn frame_with(cmd: u16, len_field: u16, payload: &[u8]) -> Vec<u8> {
        let mut f = b"HM".to_vec();
        f.extend_from_slice(&cmd.to_be_bytes());
        f.extend_from_slice(&SEQ.to_be_bytes());
        f.extend_from_slice(&State::<MODBUS>::calculate(payload).to_be_bytes());
        f.extend_from_slice(&len_field.to_be_bytes());
        f.extend_from_slice(payload);
        f
    }

    fn frame(len_field: u16, payload: &[u8]) -> Vec<u8> {
        frame_with(0xa211, len_field, payload)
    }

    /// Delivers the data one byte per read() call, like a badly fragmented TCP stream.
    struct Trickle(Vec<u8>, usize);
    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.1 >= self.0.len() || buf.is_empty() {
                return Ok(0);
            }
            buf[0] = self.0[self.1];
            self.1 += 1;
            Ok(1)
        }
    }

    #[test]
    fn read_frame_returns_payload() {
        let payload = b"\x0a\x02ab";
        let data = frame(10 + payload.len() as u16, payload);
        assert_eq!(read_frame(&mut data.as_slice(), SEQ).unwrap(), payload);
        assert_eq!(read_frame(&mut Trickle(data, 0), SEQ).unwrap(), payload);
        let legacy = frame_with(0xa203, 10 + payload.len() as u16, payload);
        assert_eq!(read_frame(&mut legacy.as_slice(), SEQ).unwrap(), payload);
    }

    #[test]
    fn read_frame_rejects_bad_crc() {
        let mut data = frame(10 + 4, b"\x0a\x02ab");
        data[7] ^= 0xff;
        assert!(read_frame(&mut data.as_slice(), SEQ).is_err());
        let mut data = frame(10 + 4, b"\x0a\x02ab");
        data[12] ^= 0x01; // corrupted payload byte
        assert!(read_frame(&mut data.as_slice(), SEQ).is_err());
    }

    #[test]
    fn read_frame_rejects_non_response_command() {
        for cmd in [0xa311, 0xa303, 0xda11, 0x0000] {
            let data = frame_with(cmd, 10 + 2, b"ab");
            assert!(read_frame(&mut data.as_slice(), SEQ).is_err());
        }
    }

    #[test]
    fn read_frame_rejects_wrong_sequence() {
        let data = frame(10 + 2, b"ab");
        assert!(read_frame(&mut data.as_slice(), SEQ + 1).is_err());
    }

    #[test]
    fn read_frame_rejects_short_or_bad_replies() {
        assert!(read_frame(&mut &b""[..], SEQ).is_err());
        assert!(read_frame(&mut &b"HM\xa2\x11"[..], SEQ).is_err());
        assert!(read_frame(&mut frame(10 + 5, b"ab").as_slice(), SEQ).is_err()); // truncated payload
        assert!(read_frame(&mut frame(4, b"").as_slice(), SEQ).is_err()); // length below header size
        let mut bad_magic = frame(10, b"");
        bad_magic[0] = b'X';
        assert!(read_frame(&mut bad_magic.as_slice(), SEQ).is_err());
    }

    #[test]
    fn no_inverters_is_stale() {
        assert!(is_stale(&response_with_links(&[])));
    }
}
