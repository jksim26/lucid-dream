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

const SECRET_PREFIXES: [&str; 10] = [
    "ld_",
    "crsr_",
    "sk-",
    "sk_",
    "ghp_",
    "gho_",
    "github_pat_",
    "xox",
    "AKIA",
    "eyJ",
];

/// Replace anything that looks like a credential with `[redacted]`, word by
/// word. Session logs become long-lived memory that every agent can read, so
/// a token pasted into a chat must not travel with them. This errs towards
/// hiding too much: a long random-looking word is assumed to be a secret.
pub fn redact(text: &str) -> String {
    let is_token_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut out = String::with_capacity(text.len());
    let mut after_bearer = false;
    let mut rest = text;
    while !rest.is_empty() {
        // Copy the separators, then judge the next run of token characters.
        let start = rest.find(is_token_char).unwrap_or(rest.len());
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest.find(|c| !is_token_char(c)).unwrap_or(rest.len());
        let run = &rest[..end];
        rest = &rest[end..];
        if run.is_empty() {
            continue;
        }
        let known = SECRET_PREFIXES.iter().any(|p| run.starts_with(p)) && run.len() >= 16;
        let random_looking = run.len() >= 32
            && run.bytes().any(|b| b.is_ascii_digit())
            && run.bytes().any(|b| b.is_ascii_alphabetic());
        out.push_str(if known || random_looking || after_bearer {
            "[redacted]"
        } else {
            run
        });
        after_bearer = run.eq_ignore_ascii_case("bearer");
    }
    out
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
    fn redacts_credentials_and_leaves_prose_alone() {
        let hex = "a4e6043401ff3ca5834015ec867ccf1ae2941f9fc7daf3b1fd04311ddd57de6b";
        assert_eq!(redact(&format!("use crsr_{hex} now")), "use [redacted] now");
        assert_eq!(
            redact(&format!("Authorization: Bearer {hex}")),
            "Authorization: Bearer [redacted]"
        );
        assert_eq!(
            redact("header is Bearer short-one."),
            "header is Bearer [redacted]."
        );
        assert_eq!(
            redact("token ld_932de123411fe6dc51af, ok"),
            "token [redacted], ok"
        );
        assert_eq!(
            redact(&format!("key={hex}")),
            format!("key={hex}").replace(hex, "[redacted]")
        );
        let prose = "Fix the login bug in src/store.rs at 2026-11-11T07:00:00Z please";
        assert_eq!(redact(prose), prose);
        assert_eq!(
            redact("commit 0fa71d6 and run 42979147512"),
            "commit 0fa71d6 and run 42979147512"
        );
    }

    #[test]
    fn checks_names() {
        assert!(valid_name("claude") && valid_name("codex-mac2"));
        for bad in ["", "Claude", "-x", "a b", "a/b", "../x"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }
}
