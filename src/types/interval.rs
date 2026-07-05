//! Oracle INTERVAL encoding and decoding.

use crate::error::{Error, Result};

const DURATION_MID: i64 = 0x8000_0000;
const DURATION_OFFSET: i32 = 60;

/// Oracle INTERVAL YEAR TO MONTH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OracleIntervalYM {
    /// Number of years.
    pub years: i32,
    /// Number of months.
    pub months: i32,
}

impl OracleIntervalYM {
    /// Create a new interval year-to-month value.
    pub fn new(years: i32, months: i32) -> Self {
        Self { years, months }
    }
}

impl std::fmt::Display for OracleIntervalYM {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let negative = self.years < 0 || self.months < 0;
        let years = self.years.abs();
        let months = self.months.abs();
        write!(
            f,
            "{}{:02}-{:02}",
            if negative { "-" } else { "+" },
            years,
            months
        )
    }
}

/// Oracle INTERVAL DAY TO SECOND.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OracleIntervalDS {
    /// Number of days.
    pub days: i32,
    /// Number of hours.
    pub hours: i32,
    /// Number of minutes.
    pub minutes: i32,
    /// Number of seconds.
    pub seconds: i32,
    /// Fractional seconds in nanoseconds.
    pub fseconds: i32,
}

impl OracleIntervalDS {
    /// Create a new interval day-to-second value.
    pub fn new(days: i32, hours: i32, minutes: i32, seconds: i32, fseconds: i32) -> Self {
        Self {
            days,
            hours,
            minutes,
            seconds,
            fseconds,
        }
    }
}

impl std::fmt::Display for OracleIntervalDS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let negative = self.days < 0
            || self.hours < 0
            || self.minutes < 0
            || self.seconds < 0
            || self.fseconds < 0;
        let days = self.days.abs();
        let hours = self.hours.abs();
        let minutes = self.minutes.abs();
        let seconds = self.seconds.abs();
        let fseconds = self.fseconds.abs();

        if fseconds == 0 {
            write!(
                f,
                "{}{:02} {:02}:{:02}:{:02}",
                if negative { "-" } else { "+" },
                days,
                hours,
                minutes,
                seconds
            )
        } else {
            write!(
                f,
                "{}{:02} {:02}:{:02}:{:02}.{:09}",
                if negative { "-" } else { "+" },
                days,
                hours,
                minutes,
                seconds,
                fseconds
            )
        }
    }
}

/// Decode an Oracle INTERVAL YEAR TO MONTH value.
pub fn decode_oracle_interval_ym(data: &[u8]) -> Result<OracleIntervalYM> {
    if data.len() < 5 {
        return Err(Error::DataConversionError(format!(
            "INTERVAL YEAR TO MONTH requires 5 bytes, got {}",
            data.len()
        )));
    }

    let years = read_excess_i32(&data[0..4])?;
    let months = data[4] as i32 - DURATION_OFFSET;
    Ok(OracleIntervalYM::new(years, months))
}

/// Decode an Oracle INTERVAL DAY TO SECOND value.
pub fn decode_oracle_interval_ds(data: &[u8]) -> Result<OracleIntervalDS> {
    if data.len() < 11 {
        return Err(Error::DataConversionError(format!(
            "INTERVAL DAY TO SECOND requires 11 bytes, got {}",
            data.len()
        )));
    }

    let days = read_excess_i32(&data[0..4])?;
    let hours = data[4] as i32 - DURATION_OFFSET;
    let minutes = data[5] as i32 - DURATION_OFFSET;
    let seconds = data[6] as i32 - DURATION_OFFSET;
    let fseconds = read_excess_i32(&data[7..11])?;
    Ok(OracleIntervalDS::new(
        days, hours, minutes, seconds, fseconds,
    ))
}

/// Encode an Oracle INTERVAL YEAR TO MONTH value.
pub fn encode_oracle_interval_ym(interval: &OracleIntervalYM) -> [u8; 5] {
    let mut out = [0u8; 5];
    out[0..4].copy_from_slice(&write_excess_i32(interval.years));
    out[4] = (interval.months + DURATION_OFFSET) as u8;
    out
}

/// Encode an Oracle INTERVAL DAY TO SECOND value.
pub fn encode_oracle_interval_ds(interval: &OracleIntervalDS) -> [u8; 11] {
    let mut out = [0u8; 11];
    out[0..4].copy_from_slice(&write_excess_i32(interval.days));
    out[4] = (interval.hours + DURATION_OFFSET) as u8;
    out[5] = (interval.minutes + DURATION_OFFSET) as u8;
    out[6] = (interval.seconds + DURATION_OFFSET) as u8;
    out[7..11].copy_from_slice(&write_excess_i32(interval.fseconds));
    out
}

fn read_excess_i32(data: &[u8]) -> Result<i32> {
    let bytes: [u8; 4] = data
        .try_into()
        .map_err(|_| Error::DataConversionError("Expected 4 bytes".to_string()))?;
    Ok((u32::from_be_bytes(bytes) as i64 - DURATION_MID) as i32)
}

fn write_excess_i32(value: i32) -> [u8; 4] {
    ((value as i64 + DURATION_MID) as u32).to_be_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_interval_ym() {
        let data = [0x80, 0x00, 0x00, 0x0a, 0x3e];
        let interval = decode_oracle_interval_ym(&data).unwrap();
        assert_eq!(interval, OracleIntervalYM::new(10, 2));
        assert_eq!(interval.to_string(), "+10-02");
        assert_eq!(encode_oracle_interval_ym(&interval), data);
    }

    #[test]
    fn test_decode_interval_ds() {
        let data = [
            0x80, 0x00, 0x00, 0x0b, 0x46, 0x45, 0x44, 0xa1, 0x14, 0xa0, 0xc0,
        ];
        let interval = decode_oracle_interval_ds(&data).unwrap();
        assert_eq!(interval, OracleIntervalDS::new(11, 10, 9, 8, 555_000_000));
        assert_eq!(interval.to_string(), "+11 10:09:08.555000000");
        assert_eq!(encode_oracle_interval_ds(&interval), data);
    }

    #[test]
    fn test_encode_negative_intervals() {
        let ym = OracleIntervalYM::new(-2, -6);
        assert_eq!(
            decode_oracle_interval_ym(&encode_oracle_interval_ym(&ym)).unwrap(),
            ym
        );

        let ds = OracleIntervalDS::new(-2, -12, -30, 0, 0);
        assert_eq!(
            decode_oracle_interval_ds(&encode_oracle_interval_ds(&ds)).unwrap(),
            ds
        );
    }
}
