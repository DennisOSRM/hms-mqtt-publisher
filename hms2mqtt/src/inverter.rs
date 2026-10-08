use crate::command::POWER_LIMIT_RANGE;
use crate::crypto::{self, CryptoError};
use crate::protos::hoymiles::{
    APPInfomationData::{APPInfoDataReqDTO, APPInfoDataResDTO},
    CommandPB::{CommandReqDTO, CommandResDTO},
    RealData::{HMSStateResponse, RealDataResDTO, Warning, WarningsRequest, WarningsResponse},
    RealDataNew::{RealDataNewReqDTO, RealDataNewResDTO},
};
use anyhow::{anyhow, Context, Result};
use chrono::Local;
use crc16::{State, MODBUS};
use log::{debug, error, info, warn};
use protobuf::Message;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const INVERTER_PORT: u16 = 10081;
pub(crate) const FRAME_HEADER_LENGTH: usize = 10;
const GCM_TAG_LENGTH: usize = 16;
const MAX_FRAME_LENGTH: usize = FRAME_HEADER_LENGTH + 4096;
const APP_INFO_COMMAND: u16 = 0xa301;
const APP_INFO_REQUEST_COMMAND: u16 = 0xa201;
const REAL_DATA_NEW_COMMAND: u16 = 0xa311;
const WARNINGS_REQUEST_COMMAND: u16 = 0xa304;
const COMMAND_RES_COMMAND: u16 = 0xa305;
const ENCRYPTION_FLAG_BIT: i64 = 25;
const ACTION_LIMIT_POWER: i32 = 8;
const ACTION_PERFORMANCE_DATA_MODE: i32 = 33;
const MAX_PAGES: i32 = 16;
/// UTC offset in seconds sent with Application Information and RealDataNew requests: a fixed
/// UTC+8, as the hoymiles-wifi library sends it
const FIXED_UTC_OFFSET: i32 = 28_800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkState {
    Unknown,
    Online,
    Offline,
}

#[derive(Clone, Debug)]
struct Capabilities {
    encrypted: bool,
    enc_rand: Option<Vec<u8>>,
    dtu_serial_number: String,
    firmware_version: i32,
}

pub struct Inverter<'a> {
    host: &'a str,
    port: u16,
    state: NetworkState,
    sequence: u16,
    page_delay: Duration,
    last_reading_stale: bool,
    capabilities: Option<Capabilities>,
    enable_performance_mode: bool,
    initialize_power_limit: Option<u8>,
    startup_command_state: StartupCommandState,
}

#[derive(Default)]
struct StartupCommandState {
    performance_mode_initialized: bool,
    power_limit_initialized: bool,
}

impl<'a> Inverter<'a> {
    pub fn with_port(host: &'a str, port: u16) -> Self {
        Self::with_port_and_options(host, port, false, None)
    }

    pub fn with_options(
        host: &'a str,
        enable_performance_mode: bool,
        initialize_power_limit: Option<u8>,
    ) -> Self {
        Self::with_port_and_options(
            host,
            INVERTER_PORT,
            enable_performance_mode,
            initialize_power_limit,
        )
    }

    fn with_port_and_options(
        host: &'a str,
        port: u16,
        enable_performance_mode: bool,
        initialize_power_limit: Option<u8>,
    ) -> Self {
        Self {
            host,
            port,
            state: NetworkState::Unknown,
            sequence: 0_u16,
            page_delay: Duration::from_secs(1),
            last_reading_stale: false,
            capabilities: None,
            enable_performance_mode,
            initialize_power_limit,
            startup_command_state: StartupCommandState::default(),
        }
    }

    pub fn state(&self) -> NetworkState {
        self.state
    }

    pub fn last_reading_stale(&self) -> bool {
        self.last_reading_stale
    }

    fn set_state(&mut self, new_state: NetworkState) {
        if self.state != new_state {
            self.state = new_state;
            info!("Inverter is {new_state:?}");
        }
    }

    pub fn update_state(&mut self) -> Option<HMSStateResponse> {
        self.last_reading_stale = false;
        if self.capabilities.is_none() {
            if let Err(error) = self.load_capabilities() {
                error!("Unable to read inverter application information: {error:#}");
                self.set_state(NetworkState::Offline);
                return None;
            }
        }
        self.initialize_startup_commands();

        let response = match self.request_telemetry() {
            Ok(response) => response,
            Err(error) if is_authentication_failure(&error) => {
                warn!("Telemetry authentication failed; refreshing application information once");
                self.capabilities = None;
                if let Err(refresh_error) = self.load_capabilities() {
                    error!("Unable to refresh inverter application information: {refresh_error:#}");
                    self.set_state(NetworkState::Offline);
                    return None;
                }
                // the DTU may have restarted, which ends its performance mode
                self.startup_command_state.performance_mode_initialized = false;
                self.initialize_startup_commands();

                match self.request_telemetry() {
                    Ok(response) => response,
                    Err(retry_error) => {
                        error!("Unable to read inverter telemetry after reinitialization: {retry_error:#}");
                        self.set_state(NetworkState::Offline);
                        return None;
                    }
                }
            }
            Err(error) => {
                error!("Unable to read inverter telemetry: {error:#}");
                self.set_state(NetworkState::Offline);
                return None;
            }
        };
        if is_stale(&response) {
            warn!("DTU reports no inverter link (link == 0), skipping stale reading");
            self.last_reading_stale = true;
            return None;
        }
        self.set_state(NetworkState::Online);
        Some(response)
    }

    fn initialize_startup_commands(&mut self) {
        if self.enable_performance_mode && !self.startup_command_state.performance_mode_initialized
        {
            info!("Enabling performance data mode");
            match self.enable_performance_data_mode() {
                Ok(()) => info!("Performance data mode enabled"),
                Err(error) => warn!("Unable to enable performance data mode: {error:#}"),
            }
            self.startup_command_state.performance_mode_initialized = true;
        }

        if let Some(power_limit) = self.initialize_power_limit {
            if !self.startup_command_state.power_limit_initialized {
                info!("Initializing inverter power limit to {power_limit}%");
                match self.set_power_limit_command(power_limit) {
                    Ok(()) => info!("Power limit initialized successfully"),
                    Err(error) => warn!("Unable to initialize inverter power limit: {error:#}"),
                }
                self.startup_command_state.power_limit_initialized = true;
            }
        }
    }

    fn load_capabilities(&mut self) -> Result<()> {
        let request = build_application_info_request()?;
        debug!("Sending A301 Application Information");
        let payload = self.send_request(
            APP_INFO_COMMAND,
            &request,
            false,
            &[APP_INFO_COMMAND, APP_INFO_REQUEST_COMMAND],
        )?;
        let response = APPInfoDataReqDTO::parse_from_bytes(&payload)
            .context("invalid Application Information protobuf")?;
        let dtu_info = response
            .dtu_info
            .as_ref()
            .context("Application Information response has no DTU information")?;
        let encrypted = ((dtu_info.dfs >> ENCRYPTION_FLAG_BIT) & 1) != 0;
        let enc_rand = if encrypted {
            let enc_rand = dtu_info.enc_rand.clone();
            if enc_rand.len() != 16 {
                return Err(anyhow!(
                    "encrypted inverter returned invalid enc_rand length {}",
                    enc_rand.len()
                ));
            }
            Some(enc_rand)
        } else {
            None
        };

        let capabilities = Capabilities {
            encrypted,
            enc_rand,
            dtu_serial_number: response.dtu_serial_number.clone(),
            firmware_version: dtu_info.dtu_sw_version,
        };
        info!(
            "Firmware version: {}, encryption: {}",
            capabilities.firmware_version,
            if capabilities.encrypted {
                "enabled"
            } else {
                "disabled"
            }
        );
        self.capabilities = Some(capabilities);
        Ok(())
    }

    fn request_telemetry(&mut self) -> Result<HMSStateResponse> {
        let capabilities = self
            .capabilities
            .clone()
            .context("inverter capabilities are not initialized")?;
        if capabilities.encrypted {
            self.request_real_data_new(&capabilities)
        } else {
            self.request_plain_real_data()
        }
    }

    fn enable_performance_data_mode(&mut self) -> Result<()> {
        let request = build_performance_data_mode_request(unix_timestamp_i32()?);
        let response = self.send_command(request)?;
        validate_command_response(&response, ACTION_PERFORMANCE_DATA_MODE)
    }

    pub fn set_power_limit(&mut self, percent: u32) -> std::result::Result<(), String> {
        if !POWER_LIMIT_RANGE.contains(&percent) {
            return Err(format!(
                "power limit must be between {} and {} %, got {percent}",
                POWER_LIMIT_RANGE.start(),
                POWER_LIMIT_RANGE.end()
            ));
        }
        let percent = percent as u8;
        self.ensure_capabilities()
            .and_then(|()| self.set_power_limit_command(percent))
            .map_err(|error| error.to_string())
    }

    pub fn fetch_warnings(&mut self) -> Option<Vec<Warning>> {
        if let Err(error) = self.ensure_capabilities() {
            debug!("could not read inverter application information for warnings: {error:#}");
            return None;
        }
        let encrypted = self
            .capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.encrypted);
        let mut warnings = Vec::new();
        let mut page = 0;
        loop {
            let now = Local::now();
            let request = WarningsRequest {
                ymd_hms: now.format("%Y-%m-%d %H:%M:%S").to_string(),
                page,
                offset: now.offset().local_minus_utc(),
                time: now.timestamp() as i32,
                ..Default::default()
            };
            let payload = match request
                .write_to_bytes()
                .map_err(anyhow::Error::new)
                .and_then(|bytes| {
                    self.send_request(
                        WARNINGS_REQUEST_COMMAND,
                        &bytes,
                        encrypted,
                        &[
                            WARNINGS_REQUEST_COMMAND,
                            response_command(WARNINGS_REQUEST_COMMAND),
                        ],
                    )
                })
                .and_then(|bytes| {
                    WarningsResponse::parse_from_bytes(&bytes)
                        .map_err(anyhow::Error::new)
                        .context("invalid warnings response protobuf")
                }) {
                Ok(response) => response,
                Err(error) => {
                    debug!("could not fetch warnings: {error:#}");
                    return None;
                }
            };
            warnings.extend(payload.warnings);
            page += 1;
            if page >= payload.page_count.clamp(1, MAX_PAGES) {
                return Some(warnings);
            }
            std::thread::sleep(self.page_delay);
        }
    }

    fn ensure_capabilities(&mut self) -> Result<()> {
        if self.capabilities.is_none() {
            self.load_capabilities()?;
        }
        Ok(())
    }

    fn set_power_limit_command(&mut self, percent: u8) -> Result<()> {
        let timestamp = unix_timestamp_i32()?;
        let request = build_power_limit_request(percent, timestamp);
        let response = self.send_command(request)?;
        validate_command_response(&response, ACTION_LIMIT_POWER)
    }

    fn send_command(&mut self, request: CommandResDTO) -> Result<CommandReqDTO> {
        let payload = request
            .write_to_bytes()
            .context("unable to serialize Hoymiles command request")?;
        let response = self.send_request(
            COMMAND_RES_COMMAND,
            &payload,
            self.capabilities
                .as_ref()
                .is_some_and(|capabilities| capabilities.encrypted),
            &[COMMAND_RES_COMMAND, response_command(COMMAND_RES_COMMAND)],
        )?;
        CommandReqDTO::parse_from_bytes(&response)
            .context("invalid Hoymiles command response protobuf")
    }

    fn request_plain_real_data(&mut self) -> Result<HMSStateResponse> {
        let mut response = self.request_plain_real_data_page(0)?;
        let pages = response.page_count.clamp(1, MAX_PAGES);
        for page in 1..pages {
            std::thread::sleep(self.page_delay);
            let next = self.request_plain_real_data_page(page)?;
            response.inverter_state.extend(next.inverter_state);
            response
                .three_phase_inverter_state
                .extend(next.three_phase_inverter_state);
            response.port_state.extend(next.port_state);
        }
        Ok(response)
    }

    fn request_plain_real_data_page(&mut self, page: i32) -> Result<HMSStateResponse> {
        let now = Local::now();
        let request = RealDataResDTO {
            ymd_hms: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            cp: page,
            offset: now.offset().local_minus_utc(),
            time: now.timestamp() as i32,
            ..Default::default()
        };
        let payload = self.send_request(
            REAL_DATA_NEW_COMMAND,
            &request.write_to_bytes()?,
            false,
            &[response_command(REAL_DATA_NEW_COMMAND)],
        )?;
        HMSStateResponse::parse_from_bytes(&payload).context("invalid RealData protobuf")
    }

    fn request_real_data_new(&mut self, capabilities: &Capabilities) -> Result<HMSStateResponse> {
        capabilities
            .enc_rand
            .as_deref()
            .context("encrypted inverter has no enc_rand")?;
        let mut request = build_real_data_new_request(0)?;
        let mut combined = self.request_real_data_new_page(&request)?;
        let package_count = combined.ap.clamp(1, MAX_PAGES);

        for package in 1..package_count {
            std::thread::sleep(self.page_delay);
            request = build_real_data_new_request(package)?;
            let response = self.request_real_data_new_page(&request)?;
            combined
                .merge_from_bytes(&response.write_to_bytes()?)
                .context("unable to combine RealDataNew protobuf pages")?;
        }

        map_real_data_new(&combined, capabilities)
    }

    fn request_real_data_new_page(
        &mut self,
        request: &RealDataNewResDTO,
    ) -> Result<RealDataNewReqDTO> {
        let request_bytes = request.write_to_bytes()?;
        debug!("Sending A311 RealDataNew");
        let payload = self.send_request(
            REAL_DATA_NEW_COMMAND,
            &request_bytes,
            true,
            &[
                REAL_DATA_NEW_COMMAND,
                response_command(REAL_DATA_NEW_COMMAND),
            ],
        )?;
        RealDataNewReqDTO::parse_from_bytes(&payload)
            .context("invalid RealDataNew protobuf")
            .inspect(|response| {
                debug!(
                    "RealDataNew telemetry: timestamp={}, dtu_power={}, sgs_power={:?}, sgs_current={:?}, sgs_voltage={:?}, pv_power={:?}, pv_current={:?}, pv_voltage={:?}, pv_daily={:?}, pv_total={:?}, modulation={:?}",
                    response.timestamp,
                    response.dtu_power,
                    response
                        .sgs_data
                        .iter()
                        .map(|value| value.active_power)
                        .collect::<Vec<_>>(),
                    response
                        .sgs_data
                        .iter()
                        .map(|value| value.current)
                        .collect::<Vec<_>>(),
                    response
                        .sgs_data
                        .iter()
                        .map(|value| value.voltage)
                        .collect::<Vec<_>>(),
                    response
                        .pv_data
                        .iter()
                        .map(|value| value.power)
                        .collect::<Vec<_>>(),
                    response
                        .pv_data
                        .iter()
                        .map(|value| value.current)
                        .collect::<Vec<_>>(),
                    response
                        .pv_data
                        .iter()
                        .map(|value| value.voltage)
                        .collect::<Vec<_>>(),
                    response
                        .pv_data
                        .iter()
                        .map(|value| value.energy_daily)
                        .collect::<Vec<_>>(),
                    response
                        .pv_data
                        .iter()
                        .map(|value| value.energy_total)
                        .collect::<Vec<_>>(),
                    response
                        .sgs_data
                        .iter()
                        .map(|value| value.modulation_index_signal)
                        .collect::<Vec<_>>()
                );
                debug!(
                    "Received RealDataNew page {} with {} SGS values and {} PV values",
                    response.cp,
                    response.sgs_data.len(),
                    response.pv_data.len()
                );
            })
    }

    fn send_request(
        &mut self,
        command: u16,
        payload: &[u8],
        encrypted: bool,
        expected_response_commands: &[u16],
    ) -> Result<Vec<u8>> {
        self.sequence = self.sequence.wrapping_add(1);
        let sequence = self.sequence;
        let message = build_frame(
            command,
            sequence,
            payload,
            encrypted,
            self.encryption_rand(),
        )?;
        debug!(
            "Sending Hoymiles command 0x{command:04x}, sequence {sequence}, frame size {}",
            message.len()
        );

        let inverter_host = format!("{}:{}", self.host, self.port);
        let address = inverter_host
            .to_socket_addrs()
            .context("unable to resolve inverter address")?
            .next()
            .context("inverter address did not resolve")?;
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))
            .context("unable to connect to inverter")?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .context("unable to set inverter write timeout")?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .context("unable to set inverter read timeout")?;
        stream
            .write_all(&message)
            .context("unable to write inverter request")?;

        read_response(
            &mut stream,
            sequence,
            encrypted,
            self.encryption_rand(),
            expected_response_commands,
        )
    }

    fn encryption_rand(&self) -> Option<&[u8]> {
        self.capabilities
            .as_ref()
            .and_then(|capabilities| capabilities.enc_rand.as_deref())
    }
}

fn is_authentication_failure(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<CryptoError>()
        .is_some_and(|crypto_error| *crypto_error == CryptoError::AuthenticationFailed)
}

fn response_command(request_command: u16) -> u16 {
    (request_command & 0x00ff) | 0xa200
}

fn build_application_info_request() -> Result<Vec<u8>> {
    let request = APPInfoDataResDTO {
        time_ymd_hms: current_time_string().into_bytes(),
        offset: FIXED_UTC_OFFSET,
        time: unix_timestamp()?,
        ..Default::default()
    };
    request
        .write_to_bytes()
        .context("unable to serialize Application Information request")
}

fn build_real_data_new_request(package: i32) -> Result<RealDataNewResDTO> {
    Ok(RealDataNewResDTO {
        time_ymd_hms: current_time_string().into_bytes(),
        offset: FIXED_UTC_OFFSET,
        time: unix_timestamp()? as i32,
        cp: package,
        ..Default::default()
    })
}

fn build_power_limit_request(percent: u8, timestamp: i32) -> CommandResDTO {
    CommandResDTO {
        time: timestamp,
        action: ACTION_LIMIT_POWER,
        package_nub: 1,
        tid: i64::from(timestamp),
        data: format!("A:{},B:0,C:0\r", i32::from(percent) * 10),
        ..Default::default()
    }
}

fn build_performance_data_mode_request(timestamp: i32) -> CommandResDTO {
    CommandResDTO {
        time: timestamp,
        action: ACTION_PERFORMANCE_DATA_MODE,
        package_nub: 1,
        ..Default::default()
    }
}

fn validate_command_response(response: &CommandReqDTO, action: i32) -> Result<()> {
    if response.action != 0 && response.action != action {
        return Err(anyhow!(
            "Hoymiles command response action mismatch: expected {action}, received {}",
            response.action
        ));
    }
    if response.err_code != 0 {
        if action == ACTION_LIMIT_POWER {
            return Err(anyhow!(
                "DTU rejected the power limit (error {})",
                response.err_code
            ));
        }
        return Err(anyhow!(
            "Hoymiles command action {action} failed with error code {}",
            response.err_code
        ));
    }
    Ok(())
}

fn current_time_string() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn unix_timestamp() -> Result<u32> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")
        .map(|duration| duration.as_secs().try_into().unwrap_or(u32::MAX))
}

fn unix_timestamp_i32() -> Result<i32> {
    unix_timestamp()?
        .try_into()
        .context("Unix timestamp does not fit into Hoymiles int32 field")
}

fn build_frame(
    command: u16,
    sequence: u16,
    payload: &[u8],
    encrypted: bool,
    enc_rand: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let wire_payload = if encrypted {
        let enc_rand = enc_rand.context("encrypted request has no enc_rand")?;
        crypto::encrypt(enc_rand, command, sequence, payload).map_err(anyhow::Error::new)?
    } else {
        payload.to_vec()
    };
    let crc_payload = if encrypted {
        wire_payload
            .get(..wire_payload.len().saturating_sub(GCM_TAG_LENGTH))
            .context("encrypted payload is shorter than the GCM tag")?
    } else {
        &wire_payload
    };
    let crc = State::<MODBUS>::calculate(crc_payload);
    let length = if encrypted {
        wire_payload
            .len()
            .checked_sub(GCM_TAG_LENGTH)
            .and_then(|length| length.checked_add(FRAME_HEADER_LENGTH))
            .context("encrypted request length overflow")?
    } else {
        wire_payload
            .len()
            .checked_add(FRAME_HEADER_LENGTH)
            .context("request length overflow")?
    };
    if length > u16::MAX as usize {
        return Err(anyhow!("request frame is too large: {length} bytes"));
    }

    let mut frame = Vec::with_capacity(FRAME_HEADER_LENGTH + wire_payload.len());
    frame.extend_from_slice(b"HM");
    frame.extend_from_slice(&command.to_be_bytes());
    frame.extend_from_slice(&sequence.to_be_bytes());
    frame.extend_from_slice(&crc.to_be_bytes());
    frame.extend_from_slice(&(length as u16).to_be_bytes());
    frame.extend_from_slice(&wire_payload);
    Ok(frame)
}

fn read_response<R: Read>(
    reader: &mut R,
    expected_sequence: u16,
    encrypted: bool,
    enc_rand: Option<&[u8]>,
    expected_commands: &[u16],
) -> Result<Vec<u8>> {
    let mut header = [0_u8; FRAME_HEADER_LENGTH];
    reader
        .read_exact(&mut header)
        .context("unable to read inverter frame header")?;
    if &header[..2] != b"HM" {
        return Err(anyhow!("invalid Hoymiles frame header"));
    }

    let command = u16::from_be_bytes([header[2], header[3]]);
    if !expected_commands.contains(&command) {
        return Err(anyhow!(
            "unexpected Hoymiles response command 0x{command:04x}"
        ));
    }
    let sequence = u16::from_be_bytes([header[4], header[5]]);
    if sequence != expected_sequence {
        return Err(anyhow!(
            "unexpected Hoymiles response sequence {sequence}, expected {expected_sequence}"
        ));
    }
    let expected_crc = u16::from_be_bytes([header[6], header[7]]);
    let declared_length = u16::from_be_bytes([header[8], header[9]]) as usize;
    if !(FRAME_HEADER_LENGTH..=MAX_FRAME_LENGTH).contains(&declared_length) {
        return Err(anyhow!("invalid Hoymiles frame length {declared_length}"));
    }

    let payload_length = declared_length - FRAME_HEADER_LENGTH;
    let mut payload = vec![0_u8; payload_length];
    reader
        .read_exact(&mut payload)
        .context("unable to read complete inverter frame payload")?;
    let tag = if encrypted {
        let mut tag = [0_u8; GCM_TAG_LENGTH];
        reader
            .read_exact(&mut tag)
            .context("unable to read complete GCM authentication tag")?;
        Some(tag)
    } else {
        None
    };

    let crc = State::<MODBUS>::calculate(&payload);
    if crc != expected_crc {
        return Err(anyhow!(
            "Hoymiles CRC mismatch: calculated 0x{crc:04x}, received 0x{expected_crc:04x}"
        ));
    }

    if encrypted {
        let mut ciphertext = payload;
        ciphertext.extend_from_slice(&tag.context("missing GCM authentication tag")?);
        let enc_rand = enc_rand.context("encrypted response has no enc_rand")?;
        crypto::decrypt(enc_rand, command, sequence, &ciphertext).map_err(anyhow::Error::new)
    } else {
        Ok(payload)
    }
}

fn checked_i32<T>(value: T, field: &str) -> Result<i32>
where
    T: TryInto<i32> + Copy + std::fmt::Display,
    T::Error: std::error::Error + Send + Sync + 'static,
{
    value
        .try_into()
        .with_context(|| format!("{field} does not fit into the legacy data model: {value}"))
}

fn map_real_data_new(
    response: &RealDataNewReqDTO,
    capabilities: &Capabilities,
) -> Result<HMSStateResponse> {
    if response.sgs_data.is_empty() {
        return Err(anyhow!("RealDataNew response contains no SGS telemetry"));
    }

    let dtu_serial_number = if !capabilities.dtu_serial_number.is_empty() {
        capabilities.dtu_serial_number.clone()
    } else if !response.device_serial_number.is_empty() {
        response.device_serial_number.clone()
    } else {
        response
            .sgs_data
            .first()
            .map(|sgs| sgs.serial_number.to_string())
            .context("RealDataNew response contains no device serial number")?
    };

    let mut mapped = HMSStateResponse {
        dtu_sn: dtu_serial_number,
        time: response.timestamp,
        page_count: response.ap,
        page: response.cp,
        version: response.firmware_version,
        pv_current_power: checked_i32(response.dtu_power, "dtu_power")?,
        pv_daily_yield: checked_i32(response.dtu_daily_energy, "dtu_daily_energy")?,
        ..Default::default()
    };

    for (index, sgs) in response.sgs_data.iter().enumerate() {
        let inverter = crate::protos::hoymiles::RealData::InverterState {
            inv_id: sgs.serial_number,
            port_id: checked_i32(index + 1, "inverter index")?,
            grid_voltage: sgs.voltage,
            grid_freq: sgs.frequency,
            pv_current_power: sgs.active_power,
            reactive_power: sgs.reactive_power,
            ac_current: sgs.current,
            power_factor: sgs.power_factor,
            temperature: sgs.temperature,
            warning_count: sgs.warning_number,
            link: sgs.link_status,
            power_limit: sgs.power_limit,
            mi_signal: sgs.modulation_index_signal,
            ..Default::default()
        };
        mapped.inverter_state.push(inverter);
    }

    for pv in &response.pv_data {
        let port = crate::protos::hoymiles::RealData::PortState {
            pv_sn: pv.serial_number,
            pv_port: pv.port_number,
            pv_vol: pv.voltage,
            pv_cur: pv.current,
            pv_power: pv.power,
            pv_energy_total: pv.energy_total,
            pv_daily_yield: pv.energy_daily,
            ..Default::default()
        };
        mapped.port_state.push(port);
    }

    Ok(mapped)
}

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
    use crate::protos::hoymiles::{
        CommandPB::CommandResDTO,
        RealData::{HMSStateResponse, InverterState, PortState, ThreePhaseInverterState},
        RealDataNew::{PvMO, RealDataNewReqDTO, SGSMO},
    };
    use crate::test_support::{fake_dtu, frame_reply, reply_to};
    use crc16::{State, MODBUS};
    use protobuf::Message;
    use std::io::{self, Read};

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
            let body = RealDataResDTO::parse_from_bytes(&request[FRAME_HEADER_LENGTH..]).unwrap();
            assert_eq!(body.cp, 0);
            assert!((body.time as i64 - Local::now().timestamp()).abs() < 60);
            assert_eq!(body.ymd_hms.len(), "2026-01-01 12:00:00".len());
            assert_eq!(body.offset, Local::now().offset().local_minus_utc());
            // Application Information uses sequence 1; telemetry starts at 2.
            assert_eq!(u16::from_be_bytes([request[4], request[5]]), i as u16 + 2);
            let payload = &request[FRAME_HEADER_LENGTH..];
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
                RealDataResDTO::parse_from_bytes(&r[FRAME_HEADER_LENGTH..])
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
                let request =
                    WarningsRequest::parse_from_bytes(&req[FRAME_HEADER_LENGTH..]).unwrap();
                assert_eq!(request.page, page);
                let mut response = WarningsResponse::new();
                response.page_count = 2;
                response.page = page;
                let mut warning = Warning::new();
                warning.code = 100 + page;
                response.warnings.push(warning);
                frame_reply(req, 0xa204, &response.write_to_bytes().unwrap())
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

    /// A command reply (0xA205) for `action` with error code `err_code`
    fn command_reply_for(req: &[u8], action: i32, err_code: i32) -> Vec<u8> {
        let response = CommandReqDTO {
            action,
            err_code,
            ..Default::default()
        };
        frame_reply(req, 0xa205, &response.write_to_bytes().unwrap())
    }

    fn fresh_reading() -> HMSStateResponse {
        response_with_links(&[1])
    }

    fn commands(requests: &[Vec<u8>]) -> Vec<u16> {
        requests
            .iter()
            .map(|r| crate::test_support::request_command(r))
            .collect()
    }

    #[test]
    fn application_information_is_requested_once() {
        let (port, dtu) = crate::test_support::fake_dtu_raw(vec![
            Box::new(|req| crate::test_support::app_info_reply(req, 770)),
            Box::new(|req| reply_to(req, &fresh_reading())),
            Box::new(|req| reply_to(req, &fresh_reading())),
        ]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        assert!(inverter.update_state().is_some());
        assert!(inverter.update_state().is_some());
        let caps = inverter.capabilities.as_ref().unwrap();
        assert!(!caps.encrypted);
        assert_eq!(caps.firmware_version, 770);
        assert_eq!(caps.dtu_serial_number, "414312345678");
        assert_eq!(commands(&dtu.join().unwrap()), [0xa301, 0xa311, 0xa311]);
    }

    #[test]
    fn invalid_application_information_is_retried_next_cycle() {
        let (port, dtu) = crate::test_support::fake_dtu_raw(vec![
            // reply without DTU information
            Box::new(|req| {
                let response = crate::protos::hoymiles::APPInfomationData::APPInfoDataReqDTO::new();
                frame_reply(req, 0xa201, &response.write_to_bytes().unwrap())
            }),
            // garbage instead of protobuf
            Box::new(|req| frame_reply(req, 0xa201, &[0xff; 4])),
            // closes the connection without answering
            Box::new(|_| Vec::new()),
            Box::new(|req| crate::test_support::app_info_reply(req, 770)),
            Box::new(|req| reply_to(req, &fresh_reading())),
        ]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        for _ in 0..3 {
            assert!(inverter.update_state().is_none());
            assert_eq!(inverter.state(), NetworkState::Offline);
            assert!(!inverter.last_reading_stale());
        }
        assert!(inverter.update_state().is_some());
        assert_eq!(inverter.state(), NetworkState::Online);
        assert_eq!(
            commands(&dtu.join().unwrap()),
            [0xa301, 0xa301, 0xa301, 0xa301, 0xa311]
        );
    }

    #[test]
    fn startup_commands_are_sent_once_before_the_first_reading() {
        let (port, dtu) = crate::test_support::fake_dtu_raw(vec![
            Box::new(|req| crate::test_support::app_info_reply(req, 770)),
            Box::new(|req| command_reply_for(req, 33, 0)),
            Box::new(|req| command_reply_for(req, 8, 0)),
            Box::new(|req| reply_to(req, &fresh_reading())),
            Box::new(|req| reply_to(req, &fresh_reading())),
        ]);
        let mut inverter = Inverter::with_port_and_options("127.0.0.1", port, true, Some(80));
        assert!(inverter.update_state().is_some());
        assert!(inverter.update_state().is_some());

        let requests = dtu.join().unwrap();
        assert_eq!(
            commands(&requests),
            [0xa301, 0xa305, 0xa305, 0xa311, 0xa311]
        );
        let performance =
            CommandResDTO::parse_from_bytes(&requests[1][FRAME_HEADER_LENGTH..]).unwrap();
        assert_eq!(performance.action, 33);
        let limit = CommandResDTO::parse_from_bytes(&requests[2][FRAME_HEADER_LENGTH..]).unwrap();
        assert_eq!(limit.action, 8);
        assert_eq!(limit.data, "A:800,B:0,C:0\r");
    }

    #[test]
    fn rejected_startup_commands_are_not_repeated() {
        let (port, dtu) = crate::test_support::fake_dtu_raw(vec![
            Box::new(|req| crate::test_support::app_info_reply(req, 770)),
            Box::new(|req| command_reply_for(req, 33, 1)),
            Box::new(|req| command_reply_for(req, 8, 1)),
            Box::new(|req| reply_to(req, &fresh_reading())),
            Box::new(|req| reply_to(req, &fresh_reading())),
        ]);
        let mut inverter = Inverter::with_port_and_options("127.0.0.1", port, true, Some(50));
        assert!(inverter.update_state().is_some());
        assert!(inverter.update_state().is_some());
        assert_eq!(
            commands(&dtu.join().unwrap()),
            [0xa301, 0xa305, 0xa305, 0xa311, 0xa311]
        );
    }

    #[test]
    fn without_options_no_startup_commands_are_sent() {
        let (port, dtu) = crate::test_support::fake_dtu_raw(vec![
            Box::new(|req| crate::test_support::app_info_reply(req, 770)),
            Box::new(|req| reply_to(req, &fresh_reading())),
        ]);
        let mut inverter = Inverter::with_options("127.0.0.1", false, None);
        inverter.port = port;
        assert!(inverter.update_state().is_some());
        assert_eq!(commands(&dtu.join().unwrap()), [0xa301, 0xa311]);
    }

    #[test]
    fn set_power_limit_sends_the_command_of_the_vendor_app() {
        let (port, dtu) = fake_dtu(vec![
            Box::new(|req| command_reply_for(req, 8, 0)),
            Box::new(|req| command_reply_for(req, 8, 1)),
        ]);
        let mut inverter = Inverter::with_port("127.0.0.1", port);
        assert_eq!(inverter.set_power_limit(55), Ok(()));
        assert!(inverter
            .set_power_limit(55)
            .unwrap_err()
            .contains("rejected"));

        let requests = dtu.join().unwrap();
        assert_eq!(&requests[0][0..4], b"HM\xa3\x05");
        let command = CommandResDTO::parse_from_bytes(&requests[0][FRAME_HEADER_LENGTH..]).unwrap();
        assert_eq!(command.action, 8);
        assert_eq!(command.dev_kind, 0);
        assert_eq!(command.package_nub, 1);
        assert_eq!(command.data, "A:550,B:0,C:0\r");
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
            Box::new(|req| frame_reply(req, 0xa211, &[0xff; 4])),
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

    struct PartialReader {
        data: Vec<u8>,
        position: usize,
        chunk_size: usize,
    }

    impl Read for PartialReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position == self.data.len() {
                return Ok(0);
            }
            let end = (self.position + self.chunk_size)
                .min(self.data.len())
                .min(self.position + buffer.len());
            let length = end - self.position;
            buffer[..length].copy_from_slice(&self.data[self.position..end]);
            self.position = end;
            Ok(length)
        }
    }

    #[test]
    fn reads_plain_frames_across_partial_tcp_reads() {
        let frame = build_frame(REAL_DATA_NEW_COMMAND, 9, b"synthetic protobuf", false, None)
            .expect("frame builds");
        let mut reader = PartialReader {
            data: frame,
            position: 0,
            chunk_size: 2,
        };

        let payload = read_response(&mut reader, 9, false, None, &[REAL_DATA_NEW_COMMAND])
            .expect("frame reads");
        assert_eq!(payload, b"synthetic protobuf");
    }

    #[test]
    fn rejects_invalid_crc_before_parsing() {
        let mut frame = build_frame(REAL_DATA_NEW_COMMAND, 9, b"synthetic protobuf", false, None)
            .expect("frame builds");
        frame[10] ^= 1;
        let mut reader = io::Cursor::new(frame);

        let error = read_response(&mut reader, 9, false, None, &[REAL_DATA_NEW_COMMAND])
            .expect_err("CRC mismatch must fail");
        assert!(error.to_string().contains("CRC mismatch"));
    }

    #[test]
    fn encrypted_frame_crc_excludes_authentication_tag() {
        let enc_rand = [0x42_u8; 16];
        let frame = build_frame(0xa311, 9, b"synthetic protobuf", true, Some(&enc_rand))
            .expect("frame builds");
        let encrypted_payload_length = frame.len() - 10 - 16;
        let crc = State::<MODBUS>::calculate(&frame[10..10 + encrypted_payload_length]);
        assert_eq!(crc, u16::from_be_bytes([frame[6], frame[7]]));
    }

    #[test]
    fn reads_and_decrypts_encrypted_frames() {
        let enc_rand = [0x42_u8; 16];
        let frame = build_frame(0xa211, 9, b"synthetic protobuf", true, Some(&enc_rand))
            .expect("frame builds");
        let mut reader = io::Cursor::new(frame);

        let payload = read_response(&mut reader, 9, true, Some(&enc_rand), &[0xa211])
            .expect("encrypted frame reads");
        assert_eq!(payload, b"synthetic protobuf");
    }

    #[test]
    fn maps_real_data_new_sgs_and_multiple_pv_ports() {
        let mut response = RealDataNewReqDTO {
            device_serial_number: "414392375232".to_string(),
            timestamp: 1_789_712_893,
            dtu_power: 1_285,
            dtu_daily_energy: 52,
            ..Default::default()
        };
        response.sgs_data.push(SGSMO {
            serial_number: 22_069_995_065_906,
            firmware_version: 1,
            voltage: 2_366,
            frequency: 4_999,
            active_power: 1_285,
            current: 54,
            power_factor: 1_000,
            temperature: 183,
            warning_number: 2,
            link_status: 1,
            power_limit: 750,
            reactive_power: 12,
            modulation_index_signal: 7_405_661,
            ..Default::default()
        });
        response.pv_data.extend([
            PvMO {
                serial_number: 22_069_995_065_906,
                port_number: 1,
                voltage: 430,
                current: 162,
                power: 698,
                energy_total: 1_295_701,
                energy_daily: 27,
                ..Default::default()
            },
            PvMO {
                serial_number: 22_069_995_065_906,
                port_number: 2,
                voltage: 421,
                current: 156,
                power: 658,
                energy_total: 1_312_058,
                energy_daily: 25,
                ..Default::default()
            },
        ]);

        let capabilities = Capabilities {
            encrypted: true,
            enc_rand: Some(vec![0x42_u8; 16]),
            dtu_serial_number: String::new(),
            firmware_version: 1,
        };
        let mapped = map_real_data_new(&response, &capabilities).expect("telemetry maps");

        assert_eq!(mapped.dtu_sn, "414392375232");
        assert_eq!(mapped.pv_current_power, 1_285);
        assert_eq!(mapped.pv_daily_yield, 52);
        assert_eq!(mapped.inverter_state.len(), 1);
        assert_eq!(mapped.inverter_state[0].ac_current, 54);
        assert_eq!(mapped.inverter_state[0].reactive_power, 12);
        assert_eq!(mapped.inverter_state[0].power_factor, 1_000);
        assert_eq!(mapped.inverter_state[0].warning_count, 2);
        assert_eq!(mapped.inverter_state[0].link, 1);
        assert_eq!(mapped.inverter_state[0].power_limit, 750);
        assert_eq!(mapped.inverter_state[0].mi_signal, 7_405_661);
        assert_eq!(mapped.port_state.len(), 2);
        assert_eq!(mapped.port_state[1].pv_port, 2);
        assert_eq!(mapped.port_state[1].pv_energy_total, 1_312_058);
    }

    #[test]
    fn stale_detection_checks_single_and_three_phase_inverters() {
        assert!(!is_stale(&response_with_links(&[0, 1])));
        assert!(is_stale(&response_with_links(&[0, 0])));

        let mut response = HMSStateResponse::new();
        assert!(super::is_stale(&response));

        let mut inverter = InverterState::new();
        inverter.link = 1;
        response.inverter_state.push(inverter);
        assert!(!super::is_stale(&response));
        response.inverter_state[0].link = 0;
        assert!(super::is_stale(&response));

        let mut inverter = ThreePhaseInverterState::new();
        inverter.link = 1;
        response.three_phase_inverter_state.push(inverter);
        assert!(!super::is_stale(&response));
    }

    #[test]
    fn power_limit_outside_the_range_is_rejected_without_connecting() {
        let mut inverter = Inverter::with_port("host.invalid", 10081);
        for percent in [0, 1, 101] {
            assert!(inverter
                .set_power_limit(percent)
                .unwrap_err()
                .contains("between 2 and 100"));
        }
    }

    #[test]
    fn serializes_performance_data_mode_request() {
        let request = build_performance_data_mode_request(1_789_712_893);

        assert_eq!(request.time, 1_789_712_893);
        assert_eq!(request.action, 33);
        assert_eq!(request.package_nub, 1);
        assert_eq!(request.tid, 0);
        assert!(request.data.is_empty());
    }

    #[test]
    fn serializes_power_limit_request() {
        let minimum = build_power_limit_request(2, 1_789_712_893);
        assert_eq!(minimum.data, "A:20,B:0,C:0\r");

        let request = build_power_limit_request(100, 1_789_712_893);

        assert_eq!(request.time, 1_789_712_893);
        assert_eq!(request.action, 8);
        assert_eq!(request.package_nub, 1);
        assert_eq!(request.tid, 1_789_712_893);
        assert_eq!(request.data, "A:1000,B:0,C:0\r");
    }

    #[test]
    fn encrypted_command_uses_existing_frame_path() {
        let enc_rand = [0x42_u8; 16];
        let request = CommandResDTO {
            action: 33,
            package_nub: 1,
            ..Default::default()
        };
        let payload = request.write_to_bytes().expect("request serializes");
        let frame = build_frame(COMMAND_RES_COMMAND, 9, &payload, true, Some(&enc_rand))
            .expect("encrypted command frame builds");
        let mut reader = io::Cursor::new(frame);

        let decrypted = read_response(
            &mut reader,
            9,
            true,
            Some(&enc_rand),
            &[COMMAND_RES_COMMAND],
        )
        .expect("encrypted command frame reads");
        let parsed = CommandResDTO::parse_from_bytes(&decrypted).expect("request parses");
        assert_eq!(parsed.action, 33);
        assert_eq!(parsed.package_nub, 1);
    }
}
