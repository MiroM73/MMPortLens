//! Times shown in the table.

use chrono::{Local, TimeZone};

/// Local clock time `HH:MM:SS` of a time in ms since the Unix epoch.
pub fn clock(ms: u64) -> String {
    match Local.timestamp_millis_opt(ms as i64) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => {
            t.format("%H:%M:%S").to_string()
        }
        chrono::LocalResult::None => String::new(),
    }
}

/// Local date and time `YYYY-MM-DD HH:MM:SS` of a time in ms since the Unix epoch.
pub fn date_time(ms: u64) -> String {
    match Local.timestamp_millis_opt(ms as i64) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => {
            t.format("%Y-%m-%d %H:%M:%S").to_string()
        }
        chrono::LocalResult::None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_format() {
        let s = clock(1_700_000_000_000);
        assert_eq!(s.len(), 8);
        assert_eq!(s.as_bytes()[2], b':');
        let d = date_time(1_700_000_000_000);
        assert_eq!(d.len(), 19);
        assert!(d.starts_with("2023-11-1"), "{d}");
    }
}
