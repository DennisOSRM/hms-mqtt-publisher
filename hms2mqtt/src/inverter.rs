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
const FRAME_HEADER_LENGTH: usize = 10;
const GCM_TAG_LENGTH: usize = 16;
const MAX_FRAME_LENGTH: usize = u16::MAX as usize;
const APP_INFO_COMMAND: u16 = 0xa301;
const APP_INFO_REQUEST_COMMAND: u16 = 0xa201;
const LEGACY_REAL_DATA_COMMAND: u16 = 0xa303;
const REAL_DATA_NEW_COMMAND: u16 = 0xa311;
const WARNINGS_REQUEST_COMMAND: u16 = 0xa304;
const COMMAND_RES_COMMAND: u16 = 0xa305;
const ENCRYPTION_FLAG_BIT: i64 = 25;
const ACTION_LIMIT_POWER: i32 = 8;
const ACTION_PERFORMANCE_DATA_MODE: i32 = 33;
const MAX_PAGES: i32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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

impl StartupCommandState {
    fn mark_performance_mode_initialized(&mut self) {
        self.performance_mode_initialized = true;
    }

    fn mark_power_limit_initialized(&mut self) {
        self.power_limit_initialized = true;
    }

    fn reset_performance_mode(&mut self) {
        self.performance_mode_initialized = false;
    }
}

impl<'a> Inverter<'a> {
    pub fn new(host: &'a str) -> Self {
        Self::with_port_and_options(host, INVERTER_PORT, false, None)
    }

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

        match self.request_telemetry() {
            Ok(response) => {
                if is_stale(&response) {
                    warn!("DTU reports no inverter link (link == 0), skipping stale reading");
                    self.last_reading_stale = true;
                    return None;
                }
                self.set_state(NetworkState::Online);
                Some(response)
            }
            Err(error) if is_authentication_failure(&error) => {
                warn!("Telemetry authentication failed; refreshing application information once");
                self.capabilities = None;
                if let Err(refresh_error) = self.load_capabilities() {
                    error!("Unable to refresh inverter application information: {refresh_error:#}");
                    self.set_state(NetworkState::Offline);
                    return None;
                }
                self.startup_command_state.reset_performance_mode();
                self.initialize_startup_commands();

                match self.request_telemetry() {
                    Ok(response) => {
                        if is_stale(&response) {
                            warn!(
                                "DTU reports no inverter link (link == 0), skipping stale reading"
                            );
                            self.last_reading_stale = true;
                            return None;
                        }
                        self.set_state(NetworkState::Online);
                        Some(response)
                    }
                    Err(retry_error) => {
                        error!("Unable to read inverter telemetry after reinitialization: {retry_error:#}");
                        self.set_state(NetworkState::Offline);
                        None
                    }
                }
            }
            Err(error) => {
                error!("Unable to read inverter telemetry: {error:#}");
                self.set_state(NetworkState::Offline);
                None
            }
        }
    }

    fn initialize_startup_commands(&mut self) {
        if self.enable_performance_mode && !self.startup_command_state.performance_mode_initialized
        {
            info!("Enabling performance data mode");
            match self.enable_performance_data_mode() {
                Ok(()) => info!("Performance data mode enabled"),
                Err(error) => warn!("Unable to enable performance data mode: {error:#}"),
            }
            self.startup_command_state
                .mark_performance_mode_initialized();
        }

        if let Some(power_limit) = self.initialize_power_limit {
            if !self.startup_command_state.power_limit_initialized {
                info!("Initializing inverter power limit to {power_limit}%");
                match self.set_power_limit_command(power_limit) {
                    Ok(()) => info!("Power limit initialized successfully"),
                    Err(error) => warn!("Unable to initialize inverter power limit: {error:#}"),
                }
                self.startup_command_state.mark_power_limit_initialized();
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
            self.request_legacy_real_data(&capabilities)
        }
    }

    fn enable_performance_data_mode(&mut self) -> Result<()> {
        let request = build_performance_data_mode_request(unix_timestamp_i32()?)?;
        let response = self.send_command(request)?;
        validate_command_response(&response, ACTION_PERFORMANCE_DATA_MODE)
    }

    pub fn set_power_limit(&mut self, percent: u32) -> std::result::Result<(), String> {
        if percent > 100 {
            return Err("power limit must be between 0 and 100 percent".to_owned());
        }
        let percent = u8::try_from(percent)
            .map_err(|_| "power limit must be between 0 and 100 percent".to_owned())?;
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
            std::thread::sleep(Duration::from_secs(1));
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
        let request = build_power_limit_request(percent, timestamp)?;
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

    fn request_legacy_real_data(
        &mut self,
        capabilities: &Capabilities,
    ) -> Result<HMSStateResponse> {
        let request = build_legacy_request()?;
        let payload = self.send_request(
            LEGACY_REAL_DATA_COMMAND,
            &request,
            false,
            &[
                LEGACY_REAL_DATA_COMMAND,
                response_command(LEGACY_REAL_DATA_COMMAND),
            ],
        )?;
        let response = HMSStateResponse::parse_from_bytes(&payload)
            .context("invalid legacy RealData protobuf")?;
        if response.dtu_sn.is_empty() && capabilities.dtu_serial_number.is_empty() {
            return Err(anyhow!(
                "legacy telemetry did not contain a DTU serial number"
            ));
        }
        Ok(response)
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
        offset: 28_800,
        time: unix_timestamp()?,
        ..Default::default()
    };
    request
        .write_to_bytes()
        .context("unable to serialize Application Information request")
}

fn build_legacy_request() -> Result<Vec<u8>> {
    let request = RealDataResDTO {
        ymd_hms: current_time_string(),
        offset: 28_800,
        time: unix_timestamp()? as i32,
        ..Default::default()
    };
    request
        .write_to_bytes()
        .context("unable to serialize legacy RealData request")
}

fn build_real_data_new_request(package: i32) -> Result<RealDataNewResDTO> {
    Ok(RealDataNewResDTO {
        time_ymd_hms: current_time_string().into_bytes(),
        offset: 28_800,
        time: unix_timestamp()? as i32,
        cp: package,
        ..Default::default()
    })
}

fn build_power_limit_request(percent: u8, timestamp: i32) -> Result<CommandResDTO> {
    if percent > 100 {
        return Err(anyhow!("power limit must be between 0 and 100 percent"));
    }

    Ok(CommandResDTO {
        time: timestamp,
        action: ACTION_LIMIT_POWER,
        package_nub: 1,
        tid: i64::from(timestamp),
        data: format!("A:{},B:0,C:0\r", i32::from(percent) * 10),
        ..Default::default()
    })
}

fn build_performance_data_mode_request(timestamp: i32) -> Result<CommandResDTO> {
    Ok(CommandResDTO {
        time: timestamp,
        action: ACTION_PERFORMANCE_DATA_MODE,
        package_nub: 1,
        ..Default::default()
    })
}

fn validate_command_response(response: &CommandReqDTO, action: i32) -> Result<()> {
    if response.action != 0 && response.action != action {
        return Err(anyhow!(
            "Hoymiles command response action mismatch: expected {action}, received {}",
            response.action
        ));
    }
    if response.err_code != 0 {
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

fn checked_u64_to_i32(value: u64, field: &str) -> Result<i32> {
    value
        .try_into()
        .with_context(|| format!("{field} does not fit into the legacy data model: {value}"))
}

fn checked_usize_to_i32(value: usize, field: &str) -> Result<i32> {
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
        pv_current_power: checked_u64_to_i32(response.dtu_power, "dtu_power")?,
        pv_daily_yield: checked_u64_to_i32(response.dtu_daily_energy, "dtu_daily_energy")?,
        ..Default::default()
    };

    for (index, sgs) in response.sgs_data.iter().enumerate() {
        let inverter = crate::protos::hoymiles::RealData::InverterState {
            inv_id: sgs.serial_number,
            port_id: checked_usize_to_i32(index + 1, "inverter index")?,
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
    use super::{
        build_frame, build_performance_data_mode_request, build_power_limit_request,
        map_real_data_new, read_response, Capabilities, COMMAND_RES_COMMAND,
        LEGACY_REAL_DATA_COMMAND,
    };
    use crate::protos::hoymiles::{
        CommandPB::CommandResDTO,
        RealData::{HMSStateResponse, InverterState, ThreePhaseInverterState},
        RealDataNew::{PvMO, RealDataNewReqDTO, SGSMO},
    };
    use crc16::{State, MODBUS};
    use protobuf::Message;
    use std::io::{self, Read};

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
        let frame = build_frame(
            LEGACY_REAL_DATA_COMMAND,
            9,
            b"synthetic protobuf",
            false,
            None,
        )
        .expect("frame builds");
        let mut reader = PartialReader {
            data: frame,
            position: 0,
            chunk_size: 2,
        };

        let payload = read_response(&mut reader, 9, false, None, &[LEGACY_REAL_DATA_COMMAND])
            .expect("frame reads");
        assert_eq!(payload, b"synthetic protobuf");
    }

    #[test]
    fn rejects_invalid_crc_before_parsing() {
        let mut frame = build_frame(
            LEGACY_REAL_DATA_COMMAND,
            9,
            b"synthetic protobuf",
            false,
            None,
        )
        .expect("frame builds");
        frame[10] ^= 1;
        let mut reader = io::Cursor::new(frame);

        let error = read_response(&mut reader, 9, false, None, &[LEGACY_REAL_DATA_COMMAND])
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
    fn public_power_limit_rejects_values_over_one_hundred_without_connecting() {
        let mut inverter = super::Inverter::new("localhost");
        assert!(inverter.set_power_limit(101).is_err());
    }

    #[test]
    fn serializes_performance_data_mode_request() {
        let request = build_performance_data_mode_request(1_789_712_893).expect("request builds");

        assert_eq!(request.time, 1_789_712_893);
        assert_eq!(request.action, 33);
        assert_eq!(request.package_nub, 1);
        assert_eq!(request.tid, 0);
        assert!(request.data.is_empty());
    }

    #[test]
    fn serializes_power_limit_request_and_validates_bounds() {
        let minimum = build_power_limit_request(0, 1_789_712_893).expect("request builds");
        assert_eq!(minimum.data, "A:0,B:0,C:0\r");

        let request = build_power_limit_request(100, 1_789_712_893).expect("request builds");

        assert_eq!(request.time, 1_789_712_893);
        assert_eq!(request.action, 8);
        assert_eq!(request.package_nub, 1);
        assert_eq!(request.tid, 1_789_712_893);
        assert_eq!(request.data, "A:1000,B:0,C:0\r");
        assert!(build_power_limit_request(101, 1_789_712_893).is_err());
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

    #[test]
    fn startup_command_state_reinitializes_only_performance_mode() {
        let mut state = super::StartupCommandState::default();

        assert!(!state.performance_mode_initialized);
        assert!(!state.power_limit_initialized);

        state.mark_performance_mode_initialized();
        state.mark_power_limit_initialized();
        assert!(state.performance_mode_initialized);
        assert!(state.power_limit_initialized);

        state.reset_performance_mode();
        assert!(!state.performance_mode_initialized);
        assert!(state.power_limit_initialized);
    }
}
