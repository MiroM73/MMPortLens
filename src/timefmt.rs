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

/// Creation time of a connection (#14): `HH:MM:SS.mmm` when it is from the same day as `now`,
/// otherwise `YYYY-MM-DD HH:MM:SS`.
pub fn stamp(ms: u64, now: u64) -> String {
    let local = |t: u64| match Local.timestamp_millis_opt(t as i64) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => Some(t),
        chrono::LocalResult::None => None,
    };
    match (local(ms), local(now)) {
        (Some(t), Some(n)) if t.date_naive() == n.date_naive() => {
            t.format("%H:%M:%S%.3f").to_string()
        }
        (Some(t), _) => t.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => String::new(),
    }
}

/// A duration: `5.123 s`, `42.5 s`, `3 min 05 s`, `2 h 07 min`, `4 d 03 h` (from RusTrace).
pub fn duration(ms: u64) -> String {
    let s = ms / 1000;
    if ms < 10_000 {
        format!("{}.{:03} s", s, ms % 1000)
    } else if ms < 60_000 {
        format!("{}.{} s", s, ms % 1000 / 100)
    } else if s < 3600 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else if s < 86_400 {
        format!("{} h {:02} min", s / 3600, s % 3600 / 60)
    } else {
        format!("{} d {:02} h", s / 86_400, s % 86_400 / 3600)
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
    fn durations() {
        assert_eq!(duration(5_123), "5.123 s");
        assert_eq!(duration(42_560), "42.5 s");
        assert_eq!(duration(185_000), "3 min 05 s");
        assert_eq!(duration(7_620_000), "2 h 07 min");
        assert_eq!(duration(4 * 86_400_000 + 3 * 3_600_000), "4 d 03 h");
    }

    #[test]
    fn clock_format() {
        let s = clock(1_700_000_000_000);
        assert_eq!(s.len(), 8);
        assert_eq!(s.as_bytes()[2], b':');
        let st = stamp(1_700_000_000_123, 1_700_000_000_500);
        assert_eq!(st.len(), 12, "{st}");
        assert!(st.ends_with(".123"), "{st}");
        // Another day: the date.
        let st = stamp(1_700_000_000_123, 1_700_000_000_123 + 3 * 86_400_000);
        assert!(st.starts_with("2023-11-1"), "{st}");
        let d = date_time(1_700_000_000_000);
        assert_eq!(d.len(), 19);
        assert!(d.starts_with("2023-11-1"), "{d}");
    }
}
