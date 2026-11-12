//! `lucid-dream upgrade`: replace this binary with the latest release, after
//! checking the download against the release's SHA-256 checksums.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::util::sha256_hex;

const REPO: &str = "jksim26/lucid-dream";
const TIMEOUT: Duration = Duration::from_secs(60);
/// No release binary is anywhere near this size; a larger download is wrong.
const MAX_DOWNLOAD: u64 = 64 * 1024 * 1024;

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// The release file for this system, if one is built for it.
pub fn asset() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("lucid-dream-macos-arm64"),
        ("linux", "x86_64") => Some("lucid-dream-linux-x64"),
        ("windows", "x86_64") => Some("lucid-dream-windows-x64.exe"),
        _ => None,
    }
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let response = agent
        .get(url)
        .set("User-Agent", "lucid-dream")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("{url} answered {code}"),
            ureq::Error::Transport(t) => format!("cannot reach {url}: {t}"),
        })?;
    let mut body = Vec::new();
    response
        .into_reader()
        .take(MAX_DOWNLOAD + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() as u64 > MAX_DOWNLOAD {
        return Err(format!("{url} is too large to be a release file"));
    }
    Ok(body)
}

/// The newest released version, without its leading `v`.
pub fn latest() -> Result<String, String> {
    let body = get(&format!(
        "https://api.github.com/repos/{REPO}/releases/latest"
    ))?;
    let release: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("bad reply from GitHub: {e}"))?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or("GitHub named no latest release")?;
    let version = tag.trim_start_matches('v');
    if numbers(version).is_none() {
        return Err(format!("unexpected release name: {tag}"));
    }
    Ok(version.to_string())
}

fn numbers(version: &str) -> Option<Vec<u64>> {
    let parts: Option<Vec<u64>> = version.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| p.len() == 3)
}

/// True when `latest` is a later version than `current`.
pub fn newer(latest: &str, current: &str) -> bool {
    match (numbers(latest), numbers(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// The checksum listed for `asset` in a `SHA256SUMS` file.
fn listed_sum<'a>(sums: &'a str, asset: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (sum, name) = line.split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == asset).then_some(sum)
    })
}

/// Put `new` where `exe` is. A running program cannot be overwritten on
/// Windows, but it can be renamed, so the old one is moved aside there.
fn swap(exe: &Path, new: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(new, fs::Permissions::from_mode(0o755))?;
    }
    if cfg!(windows) {
        // An earlier old copy may still be running (a waiting hook started
        // before the last upgrade) and so cannot be removed: use the next name.
        let moved = (0..20).find_map(|n| {
            let old = exe.with_extension(format!("old{n}.exe"));
            let _ = fs::remove_file(&old);
            fs::rename(exe, &old).ok().map(|_| old)
        });
        let Some(old) = moved else {
            return Err(std::io::Error::other(
                "could not move the running program aside",
            ));
        };
        if let Err(e) = fs::rename(new, exe) {
            let _ = fs::rename(&old, exe);
            return Err(e);
        }
        Ok(())
    } else {
        fs::rename(new, exe)
    }
}

pub fn run(check_only: bool, force: bool) -> Result<(), String> {
    let latest = latest()?;
    if !newer(&latest, CURRENT) && !force {
        println!("lucid-dream {CURRENT} is the latest release");
        return Ok(());
    }
    if check_only {
        println!(
            "lucid-dream {latest} is available (this is {CURRENT}); run `lucid-dream upgrade`"
        );
        return Ok(());
    }
    let asset = asset().ok_or(format!(
        "no release binary for this system; build it with: cargo install --git \
         https://github.com/{REPO}"
    ))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let base = format!("https://github.com/{REPO}/releases/download/v{latest}");
    println!("Downloading lucid-dream {latest}");
    let binary = get(&format!("{base}/{asset}"))?;
    let sums = String::from_utf8(get(&format!("{base}/SHA256SUMS"))?)
        .map_err(|_| "the checksum file is not text".to_string())?;
    let want = listed_sum(&sums, asset).ok_or("the release lists no checksum for this file")?;
    if !want.eq_ignore_ascii_case(&sha256_hex(&binary)) {
        return Err("checksum does not match; nothing was changed".into());
    }

    let io = |e: std::io::Error| format!("{}: {e}", exe.display());
    let new = exe.with_extension("new");
    fs::write(&new, &binary).map_err(io)?;
    if let Err(e) = swap(&exe, &new) {
        let _ = fs::remove_file(&new);
        return Err(io(e));
    }
    println!("Upgraded {CURRENT} to {latest} at {}", exe.display());
    println!(
        "A server or a waiting hook that is already running keeps the old version until it \
         restarts."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_by_number() {
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.4.2", "0.4.2"));
        assert!(!newer("0.4.1", "0.4.2"));
        assert!(!newer("latest", "0.4.2"));
    }

    #[test]
    fn finds_the_checksum_for_one_file() {
        let sums = "aa11  lucid-dream-linux-x64\nbb22 *lucid-dream-macos-arm64\n";
        assert_eq!(listed_sum(sums, "lucid-dream-macos-arm64"), Some("bb22"));
        assert_eq!(listed_sum(sums, "lucid-dream-linux-x64"), Some("aa11"));
        assert_eq!(listed_sum(sums, "lucid-dream-windows-x64.exe"), None);
    }

    #[test]
    fn swaps_the_new_file_into_place() {
        let t = crate::store::tests::Temp::new();
        let (exe, new) = (t.0.join("lucid-dream.exe"), t.0.join("lucid-dream.new"));
        fs::write(&exe, "old").unwrap();
        fs::write(&new, "new").unwrap();
        swap(&exe, &new).unwrap();
        assert_eq!(fs::read_to_string(&exe).unwrap(), "new");
        assert!(!new.exists());
    }
}
