//! Number, time and date formatting for the UI (Korean conventions).

/// `1234567` -> `1,234,567`; negative numbers keep their sign.
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `value/max`, e.g. `1,200/1,500`.
pub fn ratio(value: i64, max: i64) -> String {
    format!("{}/{}", thousands(value), thousands(max))
}

/// Play time as `H:MM:SS` (hours are not capped).
pub fn play_time(seconds: u64) -> String {
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

/// Human friendly age of a timestamp: `방금 전`, `5분 전`, `3시간 전`, `2일 전`, or the date
/// (`2026-09-26`, UTC) for anything older than 30 days or in the future. Relative wording avoids
/// needing the player's time zone, which neither the browser build nor std can provide portably.
pub fn relative_time(then: u64, now: u64) -> String {
    if then == 0 {
        return "时间未知".into();
    }
    if then > now + 60 {
        return date_utc(then);
    }
    let age = now.saturating_sub(then);
    match age {
        0..=59 => "刚刚".into(),
        60..=3599 => format!("{}分钟前", age / 60),
        3600..=86_399 => format!("{}小时前", age / 3600),
        86_400..=2_591_999 => format!("{}天前", age / 86_400),
        _ => date_utc(then),
    }
}

/// `YYYY-MM-DD` of a Unix timestamp in UTC.
pub fn date_utc(unix: u64) -> String {
    let (y, m, d) = civil_from_days((unix / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Gregorian date of a day count since 1970-01-01 (Howard Hinnant's `civil_from_days`).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `80%`.
pub fn percent(p: u8) -> String {
    format!("{p}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(-45_000), "-45,000");
        assert_eq!(thousands(i64::MIN), "-9,223,372,036,854,775,808");
        assert_eq!(ratio(1200, 1500), "1,200/1,500");
    }

    #[test]
    fn play_time_format() {
        assert_eq!(play_time(0), "0:00:00");
        assert_eq!(play_time(3661), "1:01:01");
        assert_eq!(play_time(100 * 3600 + 59), "100:00:59");
    }

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(date_utc(951_782_400), "2000-02-29");
        assert_eq!(date_utc(1_790_380_800), "2026-09-26");
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn relative_times() {
        let now = 1_790_380_800;
        assert_eq!(relative_time(0, now), "时间未知");
        assert_eq!(relative_time(now - 5, now), "刚刚");
        assert_eq!(relative_time(now - 300, now), "5分钟前");
        assert_eq!(relative_time(now - 3 * 3600, now), "3小时前");
        assert_eq!(relative_time(now - 2 * 86_400, now), "2天前");
        assert_eq!(
            relative_time(now - 40 * 86_400, now),
            date_utc(now - 40 * 86_400)
        );
        assert_eq!(relative_time(now + 86_400, now), date_utc(now + 86_400));
    }
}
