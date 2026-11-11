//! Human-friendly sizes and durations.

use std::time::{Duration, SystemTime};

/// Formats bytes with decimal units, matching what Finder / Explorer show.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else if v < 10.0 {
        format!("{v:.2} {}", UNITS[i])
    } else if v < 100.0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.0} {}", UNITS[i])
    }
}

/// Parses `500`, `100MB`, `1.5g`, `2GiB`, `750 kb` …
pub fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let num: f64 = num
        .parse()
        .map_err(|_| format!("invalid size `{s}` (try 500MB or 2GB)"))?;
    let mult: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" | "kb" => 1e3,
        "m" | "mb" => 1e6,
        "g" | "gb" => 1e9,
        "t" | "tb" => 1e12,
        "kib" => 1024.0,
        "mib" => 1024.0_f64.powi(2),
        "gib" => 1024.0_f64.powi(3),
        "tib" => 1024.0_f64.powi(4),
        other => return Err(format!("unknown size unit `{other}` (use KB, MB, GB, TB)")),
    };
    Ok((num * mult) as u64)
}

/// `1234567` → `1,234,567`.
pub fn count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

const DAY: u64 = 86_400;

/// Parses `12h`, `30d`, `2w`, `6m` (months), `1y`.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: u64 = num
        .parse()
        .map_err(|_| format!("invalid duration `{s}` (try 30d, 2w, 6m or 1y)"))?;
    let secs = match unit.trim().to_ascii_lowercase().as_str() {
        "h" => 3600,
        "" | "d" => DAY,
        "w" => 7 * DAY,
        "m" | "mo" => 30 * DAY,
        "y" => 365 * DAY,
        other => {
            return Err(format!(
                "unknown duration unit `{other}` (use h, d, w, m, y)"
            ));
        }
    };
    Ok(Duration::from_secs(n * secs))
}

pub fn age(t: Option<SystemTime>) -> Option<Duration> {
    t.and_then(|t| SystemTime::now().duration_since(t).ok())
}

/// Compact age such as `5d`, `3mo`, `2y`.
pub fn short_age(t: Option<SystemTime>) -> String {
    let Some(d) = age(t) else {
        return "—".into();
    };
    let days = d.as_secs() / DAY;
    match days {
        0 => "today".into(),
        1..=13 => format!("{days}d"),
        14..=59 => format!("{}w", days / 7),
        60..=364 => format!("{}mo", days / 30),
        _ => format!("{}y", days / 365),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1_500), "1.50 KB");
        assert_eq!(bytes(42_300_000_000), "42.3 GB");
        assert_eq!(bytes(512_000_000), "512 MB");
    }

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("500").unwrap(), 500);
        assert_eq!(parse_size("100MB").unwrap(), 100_000_000);
        assert_eq!(parse_size("1.5g").unwrap(), 1_500_000_000);
        assert_eq!(parse_size("2 GiB").unwrap(), 2 * 1024 * 1024 * 1024);
        assert!(parse_size("lots").is_err());
        assert!(parse_size("5 parsecs").is_err());
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("30d").unwrap().as_secs(), 30 * DAY);
        assert_eq!(parse_duration("2w").unwrap().as_secs(), 14 * DAY);
        assert_eq!(parse_duration("6m").unwrap().as_secs(), 180 * DAY);
        assert_eq!(parse_duration("1y").unwrap().as_secs(), 365 * DAY);
        assert_eq!(parse_duration("90").unwrap().as_secs(), 90 * DAY);
        assert!(parse_duration("soon").is_err());
    }

    #[test]
    fn short_ages() {
        let ago = |days: u64| Some(SystemTime::now() - Duration::from_secs(days * DAY + 60));
        assert_eq!(short_age(None), "—");
        assert_eq!(short_age(ago(0)), "today");
        assert_eq!(short_age(ago(5)), "5d");
        assert_eq!(short_age(ago(21)), "3w");
        assert_eq!(short_age(ago(100)), "3mo");
        assert_eq!(short_age(ago(800)), "2y");
    }
}
