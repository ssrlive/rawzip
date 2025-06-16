use crate::utils::{le_u16, le_u32, le_u64};

/// Represents the time zone of a timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeZone {
    /// UTC (Coordinated Universal Time)
    Utc,
    /// Local time (timezone unknown)
    Local,
}

/// Represents a timestamp found in a ZIP file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipDateTime {
    year: u16,
    month: u8,       // 1-12
    day: u8,         // 1-31
    hour: u8,        // 0-23
    minute: u8,      // 0-59
    second: u8,      // 0-59
    nanosecond: u32, // 0-999,999,999
    timezone: TimeZone,
}

impl std::fmt::Display for ZipDateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Write out the date and time in ISO 8601 format. RFC 3339 requires a
        // time zone, which we won't have for local times.
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )?;
        if self.nanosecond != 0 {
            write!(f, ".{:09}", self.nanosecond)?;
        }
        match self.timezone {
            TimeZone::Utc => write!(f, "Z"),
            TimeZone::Local => Ok(()),
        }
    }
}

impl ZipDateTime {
    /// Creates a ZipDateTime from date/time components.
    ///
    /// This is primarily intended for testing but can also be used in other contexts
    /// where you need to construct a ZipDateTime with specific values.
    ///
    /// # Arguments
    ///
    /// * `year` - Year (1-65535)
    /// * `month` - Month (1-12)
    /// * `day` - Day of month (1-31)
    /// * `hour` - Hour (0-23)
    /// * `minute` - Minute (0-59)
    /// * `second` - Second (0-59)
    /// * `nanosecond` - Nanosecond (0-999,999,999), defaults to 0
    /// * `timezone` - Whether this timestamp is UTC or local time
    ///
    /// # Examples
    ///
    /// ```
    /// # use rawzip::{ZipDateTime, TimeZone};
    /// let datetime = ZipDateTime::from_components(
    ///     2023, 6, 15, 14, 30, 45, 500_000_000, TimeZone::Utc
    /// );
    /// assert_eq!(datetime.year(), 2023);
    /// assert_eq!(datetime.nanosecond(), 500_000_000);
    /// assert_eq!(datetime.timezone(), TimeZone::Utc);
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn from_components(
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        nanosecond: u32,
        timezone: TimeZone,
    ) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            nanosecond,
            timezone,
        }
    }

    /// Creates a ZipDateTime from a Unix timestamp (seconds since epoch)
    pub(crate) fn from_unix(seconds: u32) -> Self {
        let (year, month, day, hour, minute, second) = unix_timestamp_to_components(seconds);
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            nanosecond: 0,
            timezone: TimeZone::Utc,
        }
    }

    /// Creates a ZipDateTime from an NTFS timestamp (100ns ticks since 1601)
    pub(crate) fn from_ntfs(ticks: u64) -> Self {
        let unix_seconds = (ticks / 10_000_000).saturating_sub(NTFS_EPOCH_OFFSET);
        let (year, month, day, hour, minute, second) =
            unix_timestamp_to_components(unix_seconds as u32);
        let nanosecond = ((ticks % 10_000_000) * 100) as u32;
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            nanosecond,
            timezone: TimeZone::Utc,
        }
    }

    /// Creates a ZipDateTime from a DosDateTime
    pub(crate) fn from_dos(dos: DosDateTime) -> Self {
        // Note: DOS timestamps with month=0 and day=0 are a gray area. Some
        // seem to normalize to 1980-01-01 while others normalize to 1979-11-30.
        Self {
            year: dos.year(),
            month: dos.month(),
            day: dos.day(),
            hour: dos.hour(),
            minute: dos.minute(),
            second: dos.second(),
            nanosecond: 0,
            timezone: TimeZone::Local,
        }
    }

    /// Returns the year component of the timestamp.
    pub fn year(&self) -> u16 {
        self.year
    }

    /// Returns the month component (1-12) of the timestamp.
    pub fn month(&self) -> u8 {
        self.month
    }

    /// Returns the day component (1-31) of the timestamp.
    pub fn day(&self) -> u8 {
        self.day
    }

    /// Returns the hour component (0-23) of the timestamp.
    pub fn hour(&self) -> u8 {
        self.hour
    }

    /// Returns the minute component (0-59) of the timestamp.
    pub fn minute(&self) -> u8 {
        self.minute
    }

    /// Returns the second component (0-59) of the timestamp.
    pub fn second(&self) -> u8 {
        self.second
    }

    /// Returns the nanosecond component (0-999,999,999) of the timestamp.
    /// For timestamps that don't support nanosecond precision, this returns 0.
    pub fn nanosecond(&self) -> u32 {
        self.nanosecond
    }

    /// Returns the timezone of this timestamp.
    pub fn timezone(&self) -> TimeZone {
        self.timezone
    }
}

/// Represents an MS-DOS timestamp with 2-second precision.
///
/// MS-DOS timestamps are stored as packed 16-bit values for date and time,
/// with a limited range from 1980 to 2107 and 2-second precision for seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DosDateTime {
    time: u16,
    date: u16,
}

impl DosDateTime {
    /// Creates a new MS-DOS datetime from packed date and time values.
    pub(crate) fn new(time: u16, date: u16) -> Self {
        Self { time, date }
    }

    /// Returns the year (1980-2107).
    pub fn year(&self) -> u16 {
        ((self.date >> 9) & 0x7f) + 1980
    }

    /// Returns the month (1-12).
    pub fn month(&self) -> u8 {
        let raw_month = ((self.date >> 5) & 0x0f) as u8;
        raw_month.clamp(1, 12)
    }

    /// Returns the day of the month (1-31).
    pub fn day(&self) -> u8 {
        let raw_day = (self.date & 0x1f) as u8;
        raw_day.clamp(1, last_day_of_month(self.year(), self.month()))
    }

    /// Returns the hour (0-23).
    pub fn hour(&self) -> u8 {
        let raw_hour = ((self.time >> 11) & 0x1f) as u8;
        raw_hour.min(23)
    }

    /// Returns the minute (0-59).
    pub fn minute(&self) -> u8 {
        let raw_minute = ((self.time >> 5) & 0x3f) as u8;
        raw_minute.min(59)
    }

    /// Returns the second (0-58, always even due to 2-second precision).
    pub fn second(&self) -> u8 {
        let raw_second = ((self.time & 0x1f) * 2) as u8;
        raw_second.min(58)
    }
}

// Extra field IDs for various timestamp formats
pub const EXTENDED_TIMESTAMP_ID: u16 = 0x5455; // "UT" - Extended timestamp
pub const UNIX_TIMESTAMP_ID: u16 = 0x5855; // "UX" - Unix timestamp (obsolete)
pub const NTFS_TIMESTAMP_ID: u16 = 0x000a; // NTFS timestamp

/// Extracts timestamp from the extra field using "last wins" strategy.
/// Returns the last valid timestamp found, or falls back to MS-DOS if none found.
/// This matches Go's zip reader behavior.
pub(crate) fn extract_best_timestamp(
    extra_field: &[u8],
    dos_time: u16,
    dos_date: u16,
) -> ZipDateTime {
    let mut pos = 0;
    let mut last_timestamp = None;

    while pos + 4 <= extra_field.len() {
        let field_id = le_u16(&extra_field[pos..pos + 2]);
        let field_size = le_u16(&extra_field[pos + 2..pos + 4]) as usize;
        pos += 4;

        if pos + field_size > extra_field.len() {
            break;
        }

        let field_data = &extra_field[pos..pos + field_size];

        match field_id {
            NTFS_TIMESTAMP_ID => {
                if let Some(timestamp) = parse_ntfs_timestamp(field_data) {
                    last_timestamp = Some(timestamp);
                }
            }
            EXTENDED_TIMESTAMP_ID => {
                if let Some(timestamp) = parse_extended_timestamp(field_data) {
                    last_timestamp = Some(timestamp);
                }
            }
            UNIX_TIMESTAMP_ID => {
                if let Some(timestamp) = parse_unix_timestamp(field_data) {
                    last_timestamp = Some(timestamp);
                }
            }
            _ => {}
        }

        pos += field_size;
    }

    // Return the last timestamp found, or fall back to MS-DOS
    last_timestamp.unwrap_or_else(|| ZipDateTime::from_dos(DosDateTime::new(dos_time, dos_date)))
}

/// Parses NTFS timestamp extra field (0x000a)
pub fn parse_ntfs_timestamp(data: &[u8]) -> Option<ZipDateTime> {
    if data.len() < 32 {
        return None;
    }

    // NTFS extra field format:
    // 4 bytes: reserved (usually 0)
    // 2 bytes: attribute tag (0x0001 for timestamps)
    // 2 bytes: attribute size (24 bytes for 3 timestamps)
    // 8 bytes: modification time
    // 8 bytes: access time
    // 8 bytes: creation time

    let tag = le_u16(&data[4..6]);
    if tag != 0x0001 {
        return None;
    }

    let size = le_u16(&data[6..8]) as usize;
    if size < 24 || data.len() < 8 + size {
        return None;
    }

    // Extract modification time (first 8 bytes of timestamp data)
    let mtime_ticks = le_u64(&data[8..16]);
    Some(ZipDateTime::from_ntfs(mtime_ticks))
}

/// Parses Extended Timestamp extra field (0x5455)
pub fn parse_extended_timestamp(data: &[u8]) -> Option<ZipDateTime> {
    if data.len() < 5 {
        return None;
    }

    let flags = data[0];
    let pos = 1;

    // Check if modification time is present (bit 0)
    if flags & 0x01 != 0 && pos + 4 <= data.len() {
        let mtime_seconds = le_u32(&data[pos..pos + 4]);
        return Some(ZipDateTime::from_unix(mtime_seconds));
    }

    None
}

/// Parses Unix timestamp extra field (0x5855) - obsolete format
pub fn parse_unix_timestamp(data: &[u8]) -> Option<ZipDateTime> {
    if data.len() < 8 {
        return None;
    }

    // Unix format has access time first, then modification time
    let mtime_seconds = le_u32(&data[4..8]);
    Some(ZipDateTime::from_unix(mtime_seconds))
}

/// Convert Unix timestamp to broken down date/time components
///
/// Based on Howard Hinnant's date library algorithm `civil_from_days`:
///
/// <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>
fn unix_timestamp_to_components(timestamp: u32) -> (u16, u8, u8, u8, u8, u8) {
    const SECONDS_PER_DAY: u32 = 86400;

    // Break timestamp into days and seconds within day
    let total_days = timestamp / SECONDS_PER_DAY;
    let seconds_in_day = timestamp % SECONDS_PER_DAY;

    // Convert seconds within day to H:M:S
    let hour = (seconds_in_day / 3600) as u8;
    let minute = ((seconds_in_day % 3600) / 60) as u8;
    let second = (seconds_in_day % 60) as u8;

    // Use a more efficient algorithm based on days since epoch
    // This uses the approach from Howard Hinnant's date library, but simplified
    let days_since_epoch = total_days as i32;

    // Shift epoch from 1970-01-01 to 0000-03-01 for easier leap year handling
    // This makes March 1st, year 0 our epoch (which aligns with leap year cycle)
    let days_since_shifted_epoch = days_since_epoch + 719468; // Days from 0000-03-01 to 1970-01-01

    // Calculate the era (400-year period)
    let era = days_since_shifted_epoch / 146097;
    let days_of_era = days_since_shifted_epoch % 146097;

    // Calculate year within the era (0-399)
    let year_of_era =
        (days_of_era - days_of_era / 1460 + days_of_era / 36524 - days_of_era / 146096) / 365;

    // Calculate the actual year
    let year = era * 400 + year_of_era;

    // Calculate day of year
    let days_before_year = year_of_era * 365 + year_of_era / 4 - year_of_era / 100;
    let day_of_year = days_of_era - days_before_year;

    // Calculate month and day
    // Months are shifted: Mar=0, Apr=1, ..., Dec=9, Jan=10, Feb=11
    let month_shifted = (5 * day_of_year + 2) / 153;
    let day_of_month = day_of_year - (153 * month_shifted + 2) / 5 + 1;

    // Convert back to normal calendar
    let (final_year, final_month) = if month_shifted < 10 {
        (year, month_shifted + 3)
    } else {
        (year + 1, month_shifted - 9)
    };

    (
        final_year as u16,
        final_month as u8,
        day_of_month as u8,
        hour,
        minute,
        second,
    )
}

// NTFS timestamp is 100-nanosecond intervals since 1601-01-01 00:00:00 UTC
pub const NTFS_EPOCH_OFFSET: u64 = 11644473600; // Seconds between 1601-01-01 and 1970-01-01

/// Returns true if the given year is a leap year.
fn is_leap(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Returns the last valid day of the given month in the given year.
fn last_day_of_month(year: u16, month: u8) -> u8 {
    if month != 2 || !is_leap(year) {
        last_day_of_month_common_year(month as usize)
    } else {
        29
    }
}

const fn last_day_of_month_common_year(m: usize) -> u8 {
    [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][m - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    impl DosDateTime {
        pub(crate) fn from_components(
            year: u16,
            month: u8,
            day: u8,
            hour: u8,
            minute: u8,
            second: u8,
        ) -> Option<Self> {
            // Validate year range (MS-DOS supports 1980-2107)
            if !(1980..=2107).contains(&year) {
                return None;
            }

            // Validate month (1-12)
            if !(1..=12).contains(&month) {
                return None;
            }

            // Validate day (1-31, simplified validation)
            if !(1..=31).contains(&day) {
                return None;
            }

            // Validate hour (0-23)
            if hour > 23 {
                return None;
            }

            // Validate minute (0-59)
            if minute > 59 {
                return None;
            }

            // Validate second (0-59)
            if second > 59 {
                return None;
            }

            // Pack the date: bits 15-9: year-1980, bits 8-5: month, bits 4-0: day
            let packed_date = ((year - 1980) << 9) | ((month as u16) << 5) | (day as u16);

            // Pack the time: bits 15-11: hour, bits 10-5: minute, bits 4-0: second/2
            let packed_time =
                ((hour as u16) << 11) | ((minute as u16) << 5) | ((second as u16) / 2);

            Some(Self {
                time: packed_time,
                date: packed_date,
            })
        }
    }

    #[test]
    fn test_dos_datetime() {
        // Test creation from components
        let datetime = DosDateTime::from_components(2023, 6, 15, 14, 30, 45).unwrap();
        assert_eq!(datetime.year(), 2023);
        assert_eq!(datetime.month(), 6);
        assert_eq!(datetime.day(), 15);
        assert_eq!(datetime.hour(), 14);
        assert_eq!(datetime.minute(), 30);
        assert_eq!(datetime.second(), 44); // Rounded down to even second
    }

    #[test]
    fn test_dos_datetime_validation() {
        // Test invalid year
        assert!(DosDateTime::from_components(1979, 1, 1, 0, 0, 0).is_none());
        assert!(DosDateTime::from_components(2108, 1, 1, 0, 0, 0).is_none());

        // Test invalid month
        assert!(DosDateTime::from_components(2020, 0, 1, 0, 0, 0).is_none());
        assert!(DosDateTime::from_components(2020, 13, 1, 0, 0, 0).is_none());

        // Test invalid day
        assert!(DosDateTime::from_components(2020, 1, 0, 0, 0, 0).is_none());
        assert!(DosDateTime::from_components(2020, 1, 32, 0, 0, 0).is_none());

        // Test invalid hour
        assert!(DosDateTime::from_components(2020, 1, 1, 24, 0, 0).is_none());

        // Test invalid minute
        assert!(DosDateTime::from_components(2020, 1, 1, 0, 60, 0).is_none());

        // Test invalid second
        assert!(DosDateTime::from_components(2020, 1, 1, 0, 0, 60).is_none());
    }

    #[test]
    fn test_dos_datetime_odd_seconds() {
        // Test that odd seconds are rounded down
        let datetime = DosDateTime::from_components(2020, 1, 1, 12, 30, 45).unwrap();
        assert_eq!(datetime.second(), 44);

        let datetime = DosDateTime::from_components(2020, 1, 1, 12, 30, 46).unwrap();
        assert_eq!(datetime.second(), 46);
    }

    #[test]
    fn test_dos_datetime_edge_cases() {
        // Test minimum date
        let datetime = DosDateTime::from_components(1980, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(datetime.year(), 1980);
        assert_eq!(datetime.month(), 1);
        assert_eq!(datetime.day(), 1);

        // Test maximum date
        let datetime = DosDateTime::from_components(2107, 12, 31, 23, 59, 58).unwrap();
        assert_eq!(datetime.year(), 2107);
        assert_eq!(datetime.month(), 12);
        assert_eq!(datetime.day(), 31);
        assert_eq!(datetime.hour(), 23);
        assert_eq!(datetime.minute(), 59);
        assert_eq!(datetime.second(), 58);
    }

    #[test]
    fn test_dos_datetime_zero_normalization() {
        // Test that zero DOS timestamp (0x0000 0x0000) is normalized to 1980-01-01 00:00:00
        let datetime = DosDateTime::new(0x0000, 0x0000);
        assert_eq!(datetime.year(), 1980);
        assert_eq!(datetime.month(), 1); // month 0 normalized to 1
        assert_eq!(datetime.day(), 1); // day 0 normalized to 1
        assert_eq!(datetime.hour(), 0);
        assert_eq!(datetime.minute(), 0);
        assert_eq!(datetime.second(), 0);

        // Test partial zero normalization - only month is zero
        let datetime = DosDateTime::new(0x0000, 0x0001); // day=1, month=0, year=1980
        assert_eq!(datetime.year(), 1980);
        assert_eq!(datetime.month(), 1); // month 0 normalized to 1
        assert_eq!(datetime.day(), 1);
        assert_eq!(datetime.hour(), 0);
        assert_eq!(datetime.minute(), 0);
        assert_eq!(datetime.second(), 0);

        // Test partial zero normalization - only day is zero
        let datetime = DosDateTime::new(0x0000, 0x0020); // day=0, month=1, year=1980
        assert_eq!(datetime.year(), 1980);
        assert_eq!(datetime.month(), 1);
        assert_eq!(datetime.day(), 1); // day 0 normalized to 1
        assert_eq!(datetime.hour(), 0);
        assert_eq!(datetime.minute(), 0);
        assert_eq!(datetime.second(), 0);
    }

    #[test]
    fn test_zip_datetime_dos() {
        let datetime = ZipDateTime::from_components(2020, 6, 15, 14, 30, 44, 0, TimeZone::Local);

        assert_eq!(datetime.year(), 2020);
        assert_eq!(datetime.month(), 6);
        assert_eq!(datetime.day(), 15);
        assert_eq!(datetime.hour(), 14);
        assert_eq!(datetime.minute(), 30);
        assert_eq!(datetime.second(), 44);
        assert_eq!(datetime.nanosecond(), 0);
        assert_eq!(datetime.timezone(), TimeZone::Local);
    }

    #[test]
    fn test_zip_datetime_unix() {
        // Unix timestamp for 2010-09-05 02:12:01 UTC
        let datetime = ZipDateTime::from_components(2010, 9, 5, 2, 12, 1, 0, TimeZone::Utc);

        assert_eq!(datetime.year(), 2010);
        assert_eq!(datetime.month(), 9);
        assert_eq!(datetime.day(), 5);
        assert_eq!(datetime.hour(), 2);
        assert_eq!(datetime.minute(), 12);
        assert_eq!(datetime.second(), 1);
        assert_eq!(datetime.nanosecond(), 0);
        assert_eq!(datetime.timezone(), TimeZone::Utc);
    }

    #[test]
    fn test_zip_datetime_ntfs() {
        // NTFS timestamp for roughly 2010-09-05 02:12:01 UTC with 500ms precision
        let datetime = ZipDateTime::from_components(2010, 9, 5, 2, 12, 1, 500000000, TimeZone::Utc);

        assert_eq!(datetime.year(), 2010);
        assert_eq!(datetime.month(), 9);
        assert_eq!(datetime.day(), 5);
        assert_eq!(datetime.hour(), 2);
        assert_eq!(datetime.minute(), 12);
        assert_eq!(datetime.second(), 1);
        assert_eq!(datetime.nanosecond(), 500000000);
        assert_eq!(datetime.timezone(), TimeZone::Utc);
    }

    #[test]
    fn test_zip_datetime_display() {
        // Test with zero nanoseconds - should omit the nanosecond part
        let datetime_no_nanos =
            ZipDateTime::from_components(2023, 6, 15, 14, 30, 42, 0, TimeZone::Utc);
        assert_eq!(format!("{}", datetime_no_nanos), "2023-06-15T14:30:42Z");

        // Test with non-zero nanoseconds - should include the nanosecond part
        let datetime_with_nanos =
            ZipDateTime::from_components(2023, 6, 15, 14, 30, 42, 500000000, TimeZone::Utc);
        assert_eq!(
            format!("{}", datetime_with_nanos),
            "2023-06-15T14:30:42.500000000Z"
        );

        // Test local time with zero nanoseconds
        let datetime_local =
            ZipDateTime::from_components(2023, 6, 15, 14, 30, 42, 0, TimeZone::Local);
        assert_eq!(format!("{}", datetime_local), "2023-06-15T14:30:42");

        // Test local time with nanoseconds
        let datetime_local_nanos =
            ZipDateTime::from_components(2023, 6, 15, 14, 30, 42, 123456789, TimeZone::Local);
        assert_eq!(
            format!("{}", datetime_local_nanos),
            "2023-06-15T14:30:42.123456789"
        );
    }

    #[test]
    fn test_parse_extended_timestamp() {
        // Extended timestamp with modification time flag and Unix timestamp
        let mut data = vec![0x01]; // Flags: modification time present
        data.extend_from_slice(&1283652721u32.to_le_bytes()); // Unix timestamp

        let result = parse_extended_timestamp(&data).unwrap();
        // Check that it's a Unix timestamp with the right components
        assert_eq!(result.year(), 2010);
        assert_eq!(result.month(), 9);
        assert_eq!(result.day(), 5);
        assert_eq!(result.hour(), 2);
        assert_eq!(result.minute(), 12);
        assert_eq!(result.second(), 1);
        assert_eq!(result.timezone(), TimeZone::Utc);
    }

    #[test]
    fn test_parse_unix_timestamp() {
        // Unix timestamp format: access time (4 bytes) + modification time (4 bytes)
        let mut data = vec![];
        data.extend_from_slice(&0u32.to_le_bytes()); // Access time (ignored)
        data.extend_from_slice(&1283652721u32.to_le_bytes()); // Modification time

        let result = parse_unix_timestamp(&data).unwrap();
        // Check that it's a Unix timestamp with the right components
        assert_eq!(result.year(), 2010);
        assert_eq!(result.month(), 9);
        assert_eq!(result.day(), 5);
        assert_eq!(result.hour(), 2);
        assert_eq!(result.minute(), 12);
        assert_eq!(result.second(), 1);
        assert_eq!(result.timezone(), TimeZone::Utc);
    }

    #[test]
    fn test_parse_ntfs_timestamp() {
        // NTFS timestamp format
        let mut data = vec![0; 4]; // Reserved
        data.extend_from_slice(&0x0001u16.to_le_bytes()); // Tag
        data.extend_from_slice(&24u16.to_le_bytes()); // Size

        // NTFS timestamp (100-nanosecond ticks since 1601-01-01)
        let ticks = (1283652721 + NTFS_EPOCH_OFFSET) * 10_000_000;
        data.extend_from_slice(&ticks.to_le_bytes()); // Modification time
        data.extend_from_slice(&0u64.to_le_bytes()); // Access time
        data.extend_from_slice(&0u64.to_le_bytes()); // Creation time

        let result = parse_ntfs_timestamp(&data).unwrap();
        // Check that it's an NTFS timestamp with the right components
        assert_eq!(result.year(), 2010);
        assert_eq!(result.month(), 9);
        assert_eq!(result.day(), 5);
        assert_eq!(result.hour(), 2);
        assert_eq!(result.minute(), 12);
        assert_eq!(result.second(), 1);
        assert_eq!(result.timezone(), TimeZone::Utc);
    }
}

#[cfg(test)]
mod property_tests {
    //! Property-based tests to verify timestamp conversion accuracy against jiff.

    use super::*;
    use quickcheck_macros::quickcheck;

    #[quickcheck]
    fn prop_unix_timestamp_conversion(unix_seconds: u32) {
        let zip_datetime = ZipDateTime::from_unix(unix_seconds);

        let Ok(timestamp) = jiff::Timestamp::from_second(unix_seconds as i64) else {
            return;
        };

        let dt = timestamp.to_zoned(jiff::tz::TimeZone::UTC);

        assert_eq!(zip_datetime.year(), dt.year() as u16, "year");
        assert_eq!(zip_datetime.month(), dt.month() as u8, "month");
        assert_eq!(zip_datetime.day(), dt.day() as u8, "day");
        assert_eq!(zip_datetime.hour(), dt.hour() as u8, "hour");
        assert_eq!(zip_datetime.minute(), dt.minute() as u8, "minute");
        assert_eq!(zip_datetime.second(), dt.second() as u8, "second");
        assert_eq!(zip_datetime.timezone(), TimeZone::Utc, "timezone");
        assert_eq!(zip_datetime.nanosecond(), 0, "nanosecond");
    }

    /// Property test: NTFS timestamp conversion should match jiff's conversion
    #[quickcheck]
    fn prop_ntfs_timestamp_conversion(ntfs_ticks: u64) {
        let zip_datetime = ZipDateTime::from_ntfs(ntfs_ticks);

        // Convert NTFS ticks to Unix timestamp for jiff
        // NTFS ticks are 100-nanosecond intervals since 1601-01-01
        let unix_seconds = (ntfs_ticks / 10_000_000).saturating_sub(NTFS_EPOCH_OFFSET);
        let nanoseconds = ((ntfs_ticks % 10_000_000) * 100) as u32;

        if unix_seconds > u32::MAX as u64 {
            return;
        }

        let Ok(jiff_timestamp) = jiff::Timestamp::new(unix_seconds as i64, nanoseconds as i32)
        else {
            return;
        };

        let dt = jiff_timestamp.to_zoned(jiff::tz::TimeZone::UTC);

        assert_eq!(zip_datetime.year(), dt.year() as u16, "year");
        assert_eq!(zip_datetime.month(), dt.month() as u8, "month");
        assert_eq!(zip_datetime.day(), dt.day() as u8, "day");
        assert_eq!(zip_datetime.hour(), dt.hour() as u8, "hour");
        assert_eq!(zip_datetime.minute(), dt.minute() as u8, "minute");
        assert_eq!(zip_datetime.second(), dt.second() as u8, "second");
        assert_eq!(zip_datetime.timezone(), TimeZone::Utc, "timezone");
        assert_eq!(zip_datetime.nanosecond(), nanoseconds, "nanosecond");
    }

    /// Property test: DOS timestamp conversion should always produce valid jiff datetimes
    #[quickcheck]
    fn prop_dos_timestamp_always_valid(dos_time: u16, dos_date: u16) {
        let dos_datetime = DosDateTime::new(dos_time, dos_date);
        let zip_datetime = ZipDateTime::from_dos(dos_datetime);

        // Create jiff datetime - this should never fail with our normalization
        let dt = jiff::civil::DateTime::new(
            zip_datetime.year() as i16,
            zip_datetime.month() as i8,
            zip_datetime.day() as i8,
            zip_datetime.hour() as i8,
            zip_datetime.minute() as i8,
            zip_datetime.second() as i8,
            0, // nanosecond
        )
        .unwrap();

        // Verify the components match what we expect
        assert_eq!(zip_datetime.year(), dt.year() as u16, "year");
        assert_eq!(zip_datetime.month(), dt.month() as u8, "month");
        assert_eq!(zip_datetime.day(), dt.day() as u8, "day");
        assert_eq!(zip_datetime.hour(), dt.hour() as u8, "hour");
        assert_eq!(zip_datetime.minute(), dt.minute() as u8, "minute");
        assert_eq!(zip_datetime.second(), dt.second() as u8, "second");
    }
}
