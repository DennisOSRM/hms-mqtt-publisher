use crate::protos::hoymiles::RealData::{
    CommandRequest, CommandResponse, HMSStateResponse, RealDataResDTO, Warning, WarningsRequest,
    WarningsResponse,
};
use crc16::{State, MODBUS};
use log::{debug, error, info, warn};
use protobuf::Message;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::Duration;

use chrono::Local;

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
    page_delay: Duration,
    last_reading_stale: bool,
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
            page_delay: Duration::from_secs(1),
            last_reading_stale: false,
        }
    }

    pub fn state(&self) -> NetworkState {
        self.state
    }

    /// Whether the DTU answered the last request with stale data, i.e. it currently doesn't
    /// read the inverter, typically after being polled too often.
    pub fn last_reading_stale(&self) -> bool {
        self.last_reading_stale
    }

    fn set_state(&mut self, new_state: NetworkState) {
        if self.state != new_state {
            self.state = new_state;
            info!("Inverter is {new_state:?}");
        }
    }

    /// Fetches the real-time data, requesting further pages if the DTU splits its reply.
    pub fn update_state(&mut self) -> Option<HMSStateResponse> {
        self.last_reading_stale = false;
        let mut response = match self.request_page(0) {
            Ok(response) => response,
            Err(e) => {
                debug!("{e}");
                self.set_state(NetworkState::Offline);
                return None;
            }
        };
        let pages = response.page_count.clamp(1, MAX_PAGES);
        for page in 1..pages {
            // the app waits between pages as well
            thread::sleep(self.page_delay);
            match self.request_page(page) {
                Ok(next) => {
                    response.inverter_state.extend(next.inverter_state);
                    response
                        .three_phase_inverter_state
                        .extend(next.three_phase_inverter_state);
                    response.port_state.extend(next.port_state);
                }
                Err(e) => {
                    debug!("page {page} of {pages}: {e}");
                    self.set_state(NetworkState::Offline);
                    return None;
                }
            }
        }

        if is_stale(&response) {
            // The DTU answered, but its last read of the inverter(s) failed; the values
            // are a repeat of the previous reading and must not be published as fresh.
            warn!("DTU reports no inverter link (link == 0), skipping stale reading");
            self.last_reading_stale = true;
            return None;
        }
        self.set_state(NetworkState::Online);
        Some(response)
    }

    /// One request/reply exchange for page `page` of the real-time data (0xA311).
    fn request_page(&mut self, page: i32) -> Result<HMSStateResponse, String> {
        let now = Local::now();
        let mut request = RealDataResDTO::new();
        request.ymd_hms = now.format("%Y-%m-%d %H:%M:%S").to_string();
        request.time = now.timestamp() as i32;
        request.offset = now.offset().local_minus_utc();
        request.cp = page;
        let payload = self.exchange(REAL_DATA_REQUEST, &request)?;
        HMSStateResponse::parse_from_bytes(&payload).map_err(|e| e.to_string())
    }

    /// Fetches the warnings the DTU reports (0xA304). Every request counts towards the
    /// DTU's rate limit, so this should be called rarely, right after a reading.
    pub fn fetch_warnings(&mut self) -> Option<Vec<Warning>> {
        let mut warnings = Vec::new();
        let mut page = 0;
        loop {
            let now = Local::now();
            let mut request = WarningsRequest::new();
            request.ymd_hms = now.format("%Y-%m-%d %H:%M:%S").to_string();
            request.time = now.timestamp() as i32;
            request.offset = now.offset().local_minus_utc();
            request.page = page;
            let response = match self
                .exchange(WARNINGS_REQUEST, &request)
                .and_then(|payload| {
                    WarningsResponse::parse_from_bytes(&payload).map_err(|e| e.to_string())
                }) {
                Ok(response) => response,
                Err(e) => {
                    debug!("could not fetch warnings: {e}");
                    return None;
                }
            };
            warnings.extend(response.warnings);
            page += 1;
            if page >= response.page_count.clamp(1, MAX_PAGES) {
                return Some(warnings);
            }
            thread::sleep(self.page_delay);
        }
    }

    /// Sets the active power limit of all inverters of the DTU, in percent of their rated
    /// power (command action 8, as the vendor app sends it).
    pub fn set_power_limit(&mut self, percent: u32) -> Result<(), String> {
        let now = Local::now().timestamp();
        let mut request = CommandRequest::new();
        request.time = now as i32;
        request.action = ACTION_POWER_LIMIT;
        request.dev_kind = 0;
        request.package_nub = 1;
        request.tid = now;
        // tenths of a percent for phase A; B and C are only used by three-phase setups
        request.data = format!("A:{},B:0,C:0\r", percent * 10).into_bytes();
        let payload = self.exchange(COMMAND_REQUEST, &request)?;
        let response = CommandResponse::parse_from_bytes(&payload).map_err(|e| e.to_string())?;
        if response.err_code != 0 {
            return Err(format!(
                "DTU rejected the power limit (error {})",
                response.err_code
            ));
        }
        info!("power limit set to {percent} %");
        Ok(())
    }

    /// Sends `request` with command `cmd` and returns the payload of the reply.
    fn exchange(&mut self, cmd: u16, request: &impl Message) -> Result<Vec<u8>, String> {
        self.sequence = self.sequence.wrapping_add(1);

        let request_as_bytes = request.write_to_bytes().expect("serialize to bytes");
        let crc16 = State::<MODBUS>::calculate(&request_as_bytes);
        let len = request_as_bytes.len() as u16 + HEADER_LEN as u16;

        // compose request message
        let mut message = Vec::new();
        message.extend_from_slice(b"HM");
        message.extend_from_slice(&cmd.to_be_bytes());
        message.extend_from_slice(&self.sequence.to_be_bytes());
        message.extend_from_slice(&crc16.to_be_bytes());
        message.extend_from_slice(&len.to_be_bytes());
        message.extend_from_slice(&request_as_bytes);

        // name resolution problems are configuration errors, so they are logged prominently
        let address = match (self.host, self.port).to_socket_addrs() {
            Ok(mut addresses) => addresses.next(),
            Err(e) => {
                error!("Unable to resolve domain: {e}");
                None
            }
        }
        .ok_or_else(|| format!("no address for {}", self.host))?;

        let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))
            .map_err(|e| format!("could not connect: {e}"))?;
        if let Err(e) = stream.set_write_timeout(Some(Duration::new(5, 0))) {
            warn!("could not set write timeout: {e}");
        }
        if let Err(e) = stream.set_read_timeout(Some(Duration::new(5, 0))) {
            warn!("could not set read timeout: {e}");
        }
        stream.write_all(&message).map_err(|e| e.to_string())?;

        read_frame(&mut stream, self.sequence)
    }
}

/// Real-time data request, answered with 0xA211 (RealDataNew in the S-Miles Installer app)
const REAL_DATA_REQUEST: u16 = 0xa311;
/// Warnings request, answered with 0xA204 (WarnData in the S-Miles Installer app)
const WARNINGS_REQUEST: u16 = 0xa304;
/// Control command, answered with 0xA205 (CommandPB in the S-Miles Installer app)
const COMMAND_REQUEST: u16 = 0xa305;
const ACTION_POWER_LIMIT: i32 = 8;
/// Upper bound for the number of pages requested per reading
const MAX_PAGES: i32 = 16;

pub(crate) const HEADER_LEN: usize = 10;
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

/// A reading is stale when no inverter (single- or three-phase) reports a working link to the DTU.
pub fn is_stale(response: &HMSStateResponse) -> bool {
    response
        .inverter_state
        .iter()
        .map(|inverter| inverter.link)
        .chain(
            response
                .three_phase_inverter_state
                .iter()
                .map(|inverter| inverter.link),
        )
        .all(|link| link == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protos::hoymiles::RealData::{InverterState, PortState, ThreePhaseInverterState};
    use crate::test_support::{fake_dtu, reply_to};

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
            assert_eq!(&request[0..4], b"HM\xa3\x11");
            let body = RealDataResDTO::parse_from_bytes(&request[HEADER_LEN..]).unwrap();
            assert_eq!(body.cp, 0);
            assert!((body.time as i64 - Local::now().timestamp()).abs() < 60);
            assert_eq!(body.ymd_hms.len(), "2026-01-01 12:00:00".len());
            assert_eq!(body.offset, Local::now().offset().local_minus_utc());
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

    /// A reply with `inverters` linked inverters with 2 ports each, as page `page` of `pages`
    fn page(dtu_sn: &str, pages: i32, page: i32, inverters: i64) -> HMSStateResponse {
        let mut response = response_with_links(&vec![1; inverters as usize]);
        response.dtu_sn = dtu_sn.to_string();
        response.page_count = pages;
        response.page = page;
        for (n, inverter) in response.inverter_state.iter_mut().enumerate() {
            inverter.inv_id = 100 * page as i64 + n as i64;
        }
        for _ in 0..2 * inverters {
            response.port_state.push(PortState::new());
        }
        response
    }

    #[test]
    fn update_state_requests_and_merges_all_pages() {
        let (port, dtu) = fake_dtu(vec![
            Box::new(|req| reply_to(req, &page("dtu", 2, 0, 2))),
            Box::new(|req| reply_to(req, &page("dtu", 2, 1, 1))),
        ]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        inverter.page_delay = Duration::ZERO;

        let reading = inverter.update_state().expect("fresh reading");
        let ids: Vec<i64> = reading.inverter_state.iter().map(|i| i.inv_id).collect();
        assert_eq!(ids, [0, 1, 100]);
        assert_eq!(reading.port_state.len(), 6);

        let requests = dtu.join().unwrap();
        let pages: Vec<i32> = requests
            .iter()
            .map(|r| {
                RealDataResDTO::parse_from_bytes(&r[HEADER_LEN..])
                    .unwrap()
                    .cp
            })
            .collect();
        assert_eq!(pages, [0, 1]);
    }

    #[test]
    fn update_state_fails_if_a_page_is_missing() {
        let (port, dtu) = fake_dtu(vec![
            Box::new(|req| reply_to(req, &page("dtu", 2, 0, 1))),
            Box::new(|_| Vec::new()),
        ]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        inverter.page_delay = Duration::ZERO;
        assert!(inverter.update_state().is_none());
        assert_eq!(inverter.state(), NetworkState::Offline);
        dtu.join().unwrap();
    }

    #[test]
    fn fetch_warnings_requests_all_pages() {
        let warnings_page = |page: i32| {
            move |req: &[u8]| {
                assert_eq!(&req[0..4], b"HM\xa3\x04");
                let request = WarningsRequest::parse_from_bytes(&req[HEADER_LEN..]).unwrap();
                assert_eq!(request.page, page);
                let mut response = WarningsResponse::new();
                response.page_count = 2;
                response.page = page;
                let mut warning = Warning::new();
                warning.code = 100 + page;
                response.warnings.push(warning);
                let payload = response.write_to_bytes().unwrap();
                let mut f = b"HM\xa2\x04".to_vec();
                f.extend_from_slice(&req[4..6]);
                f.extend_from_slice(&State::<MODBUS>::calculate(&payload).to_be_bytes());
                f.extend_from_slice(&(HEADER_LEN as u16 + payload.len() as u16).to_be_bytes());
                f.extend_from_slice(&payload);
                f
            }
        };
        let (port, dtu) = fake_dtu(vec![Box::new(warnings_page(0)), Box::new(warnings_page(1))]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        inverter.page_delay = Duration::ZERO;
        let codes: Vec<i32> = inverter
            .fetch_warnings()
            .expect("warnings")
            .iter()
            .map(|w| w.code)
            .collect();
        assert_eq!(codes, [100, 101]);
        dtu.join().unwrap();
    }

    #[test]
    fn fetch_warnings_fails_gracefully() {
        let (port, dtu) = fake_dtu(vec![Box::new(|_| Vec::new())]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        assert!(inverter.fetch_warnings().is_none());
        dtu.join().unwrap();
    }

    /// A DTU reply to a command request with the given error code
    fn command_reply(req: &[u8], err_code: i32) -> Vec<u8> {
        let mut response = CommandResponse::new();
        response.err_code = err_code;
        response.action = 8;
        let payload = response.write_to_bytes().unwrap();
        let mut f = b"HM\xa2\x05".to_vec();
        f.extend_from_slice(&req[4..6]);
        f.extend_from_slice(&State::<MODBUS>::calculate(&payload).to_be_bytes());
        f.extend_from_slice(&(HEADER_LEN as u16 + payload.len() as u16).to_be_bytes());
        f.extend_from_slice(&payload);
        f
    }

    #[test]
    fn set_power_limit_sends_the_command_of_the_vendor_app() {
        let (port, dtu) = fake_dtu(vec![
            Box::new(|req| command_reply(req, 0)),
            Box::new(|req| command_reply(req, 1)),
        ]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        assert_eq!(inverter.set_power_limit(55), Ok(()));
        assert!(inverter
            .set_power_limit(55)
            .unwrap_err()
            .contains("rejected"));

        let requests = dtu.join().unwrap();
        assert_eq!(&requests[0][0..4], b"HM\xa3\x05");
        let command = CommandRequest::parse_from_bytes(&requests[0][HEADER_LEN..]).unwrap();
        assert_eq!(command.action, 8);
        assert_eq!(command.dev_kind, 0);
        assert_eq!(command.package_nub, 1);
        assert_eq!(command.data, b"A:550,B:0,C:0\r");
        assert_eq!(command.tid, command.time as i64);
    }

    #[test]
    fn update_state_skips_stale_reading() {
        let stale = response_with_links(&[0]);
        let (port, dtu) = fake_dtu(vec![Box::new(move |req| reply_to(req, &stale))]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        assert!(inverter.update_state().is_none());
        assert_ne!(inverter.state(), NetworkState::Online);
        assert!(inverter.last_reading_stale());
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
    fn linked_three_phase_inverter_is_fresh() {
        let mut response = response_with_links(&[]);
        let mut inverter = ThreePhaseInverterState::new();
        inverter.link = 1;
        response.three_phase_inverter_state.push(inverter);
        assert!(!is_stale(&response));
        response.three_phase_inverter_state[0].link = 0;
        assert!(is_stale(&response));
    }

    #[test]
    fn no_inverters_is_stale() {
        assert!(is_stale(&response_with_links(&[])));
    }
}
