/// Commands received via MQTT for the inverter
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    /// Active power limit in percent of the rated power, for all inverters of the DTU
    SetPowerLimit(u32),
}

/// Range the vendor app allows for the power limit
pub const POWER_LIMIT_RANGE: std::ops::RangeInclusive<u32> = 2..=100;

/// Parses a power limit command payload like "50" or "50.0" (percent)
pub fn parse_power_limit(payload: &[u8]) -> Result<Command, String> {
    let text = String::from_utf8_lossy(payload);
    let value: f64 = text
        .trim()
        .parse()
        .map_err(|_| format!("power limit must be a number of percent, got '{text}'"))?;
    let percent = value.round();
    if !value.is_finite() || !POWER_LIMIT_RANGE.contains(&(percent as u32)) || percent < 0.0 {
        return Err(format!(
            "power limit must be between {} and {} %, got {text}",
            POWER_LIMIT_RANGE.start(),
            POWER_LIMIT_RANGE.end()
        ));
    }
    Ok(Command::SetPowerLimit(percent as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_limits() {
        assert_eq!(parse_power_limit(b"50"), Ok(Command::SetPowerLimit(50)));
        assert_eq!(
            parse_power_limit(b" 75.4\n"),
            Ok(Command::SetPowerLimit(75))
        );
        assert_eq!(parse_power_limit(b"100"), Ok(Command::SetPowerLimit(100)));
        assert_eq!(parse_power_limit(b"2"), Ok(Command::SetPowerLimit(2)));
    }

    #[test]
    fn rejects_invalid_limits() {
        for payload in [&b"0"[..], b"1", b"101", b"-5", b"abc", b"", b"NaN", b"inf"] {
            assert!(parse_power_limit(payload).is_err(), "{payload:?}");
        }
    }
}
