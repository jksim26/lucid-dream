//! Small shared helpers: UTC timestamps, hashing, names, git.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// A UTC moment, formatted without a date library.
pub struct Utc {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
}

impl Utc {
    pub fn now() -> Utc {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Utc::from_unix(secs)
    }

    pub fn from_unix(secs: i64) -> Utc {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Utc {
            year,
            month,
            day,
            hour: rem / 3_600,
            minute: rem % 3_600 / 60,
            second: rem % 60,
        }
    }

    /// 2026-11-11
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// 2026-11-11T07:00:00Z
    pub fn iso(&self) -> String {
        format!(
            "{}T{:02}:{:02}:{:02}Z",
            self.date(),
            self.hour,
            self.minute,
            self.second
        )
    }

    /// 20261111T070000Z, safe in a file name
    pub fn compact(&self) -> String {
        format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Agent names: 1 to 32 of a-z, 0-9 and dash, starting with a letter or digit.
pub fn valid_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
}

/// Run git in `root`, returning stdout or git's error text.
pub fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_moments() {
        assert_eq!(Utc::from_unix(0).iso(), "1970-01-01T00:00:00Z");
        assert_eq!(Utc::from_unix(1_700_000_000).iso(), "2023-11-14T22:13:20Z");
        assert_eq!(Utc::from_unix(951_782_400).date(), "2000-02-29");
        assert_eq!(Utc::from_unix(1_700_000_000).compact(), "20231114T221320Z");
    }

    #[test]
    fn checks_names() {
        assert!(valid_name("claude") && valid_name("codex-mac2"));
        for bad in ["", "Claude", "-x", "a b", "a/b", "../x"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
