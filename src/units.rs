//! Byte sizes, durations and ages in the units people read on their own disk tools.
//! GB here is 10^9 bytes, the unit macOS Finder, `df -H` and most vendors print.

use std::time::{Duration, SystemTime};

pub fn bytes(n: u64) -> String {
    const K: f64 = 1000.0;
    let f = n as f64;
    if f < K {
        format!("{n} B")
    } else if f < K * K {
        format!("{:.0} KB", f / K)
    } else if f < K * K * K {
        format!("{:.1} MB", f / (K * K))
    } else if f < K * K * K * K {
        format!("{:.1} GB", f / (K * K * K))
    } else {
        format!("{:.2} TB", f / (K * K * K * K))
    }
}

/// Parse "10GB", "500 MB", "1.5G", "2TB", "8%" (percent is returned separately).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Amount {
    Bytes(u64),
    Percent(f64),
}

impl Amount {
    pub fn resolve(self, total: u64) -> u64 {
        match self {
            Amount::Bytes(b) => b,
            Amount::Percent(p) => ((total as f64) * p / 100.0) as u64,
        }
    }
    pub fn label(self) -> String {
        match self {
            Amount::Bytes(b) => bytes(b),
            Amount::Percent(p) => format!("{p}%"),
        }
    }
}

pub fn parse_amount(s: &str) -> Result<Amount, String> {
    let t = s.trim().replace(' ', "").to_ascii_lowercase();
    if let Some(p) = t.strip_suffix('%') {
        let v: f64 = p
            .parse()
            .map_err(|_| format!("'{s}' is not a percentage like 5%"))?;
        if !(0.0..=100.0).contains(&v) {
            return Err(format!("'{s}': a percentage must be between 0% and 100%"));
        }
        return Ok(Amount::Percent(v));
    }
    let idx = t.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(t.len());
    let (num, unit) = t.split_at(idx);
    let v: f64 = num
        .parse()
        .map_err(|_| format!("'{s}' is not a size; use something like 10GB, 500MB or 5%"))?;
    let mul: f64 = match unit {
        "" | "b" => 1.0,
        "k" | "kb" => 1e3,
        "m" | "mb" => 1e6,
        "g" | "gb" => 1e9,
        "t" | "tb" => 1e12,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        "tib" => 1024.0f64.powi(4),
        _ => {
            return Err(format!(
                "'{s}': unknown unit '{unit}' (use B, KB, MB, GB, TB or %)"
            ));
        }
    };
    if v < 0.0 {
        return Err(format!("'{s}' is negative"));
    }
    Ok(Amount::Bytes((v * mul) as u64))
}

/// Parse "30d", "12h", "90m", "45s", "2w".
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let t = s.trim().to_ascii_lowercase();
    let idx = t.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(t.len());
    let (num, unit) = t.split_at(idx);
    let v: f64 = num
        .parse()
        .map_err(|_| format!("'{s}' is not a duration; use something like 30d, 12h, 10s"))?;
    let secs = match unit {
        "s" | "" => v,
        "m" | "min" => v * 60.0,
        "h" => v * 3600.0,
        "d" => v * 86400.0,
        "w" => v * 7.0 * 86400.0,
        _ => {
            return Err(format!(
                "'{s}': unknown unit '{unit}' (use s, m, h, d or w)"
            ));
        }
    };
    if secs < 0.0 {
        return Err(format!("'{s}' is negative"));
    }
    Ok(Duration::from_secs_f64(secs))
}

pub fn age_secs(t: SystemTime, now: SystemTime) -> u64 {
    now.duration_since(t).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn age(secs: u64) -> String {
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86399 => format!("{} h", secs / 3600),
        86400..=5_183_999 => format!("{} d", secs / 86400),
        _ => format!("{} mo", secs / (30 * 86400)),
    }
}

pub fn duration_label(d: Duration) -> String {
    let s = d.as_secs();
    if s % 86400 == 0 && s >= 86400 {
        format!("{} d", s / 86400)
    } else if s % 3600 == 0 && s >= 3600 {
        format!("{} h", s / 3600)
    } else if s % 60 == 0 && s >= 60 {
        format!("{} min", s / 60)
    } else {
        format!("{s} s")
    }
}

pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes() {
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(33_100_000_000), "33.1 GB");
        assert_eq!(bytes(1_500_000), "1.5 MB");
        assert_eq!(parse_amount("10GB").unwrap(), Amount::Bytes(10_000_000_000));
        assert_eq!(parse_amount("1.5 g").unwrap(), Amount::Bytes(1_500_000_000));
        assert_eq!(parse_amount("5%").unwrap(), Amount::Percent(5.0));
        assert_eq!(parse_amount("1GiB").unwrap(), Amount::Bytes(1 << 30));
        assert!(parse_amount("ten").is_err());
        assert!(parse_amount("5 parsecs").is_err());
        assert!(parse_amount("120%").is_err());
        assert_eq!(Amount::Percent(10.0).resolve(1000), 100);
    }
    #[test]
    fn durations() {
        assert_eq!(parse_duration("30d").unwrap().as_secs(), 30 * 86400);
        assert_eq!(parse_duration("10s").unwrap().as_secs(), 10);
        assert_eq!(parse_duration("90m").unwrap().as_secs(), 5400);
        assert!(parse_duration("3 fortnights").is_err());
        assert_eq!(duration_label(Duration::from_secs(30 * 86400)), "30 d");
        assert_eq!(age(7200), "2 h");
        assert_eq!(age(3 * 86400), "3 d");
        assert_eq!(thousands(9412), "9,412");
        assert_eq!(thousands(123), "123");
    }
}
