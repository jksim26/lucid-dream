//! `lucid-dream upgrade`: replace this binary with the latest release, after
//! checking the download against the release's SHA-256 checksums.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

/// A release, and whether its file for this system has been uploaded yet.
/// GitHub shows a new release a few minutes before its files are built.
pub struct Release {
    pub version: String,
    pub ready: bool,
}

/// True when a release's reply from GitHub lists `asset` and the checksums.
fn has_files(release: &serde_json::Value, asset: &str) -> bool {
    let listed = |name: &str| {
        release["assets"]
            .as_array()
            .is_some_and(|files| files.iter().any(|file| file["name"] == name))
    };
    listed(asset) && listed("SHA256SUMS")
}

pub const NOT_READY: &str = "was released moments ago and its file for this system is not \
                             there yet; try again in a few minutes";

/// The newest released version, without its leading `v`.
pub fn latest() -> Result<Release, String> {
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
    Ok(Release {
        version: version.to_string(),
        // A system with no release file builds from source: nothing to wait for.
        ready: asset().is_none_or(|asset| has_files(&release, asset)),
    })
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
    let Release {
        version: latest,
        ready,
    } = latest()?;
    if !newer(&latest, CURRENT) && !force {
        println!("lucid-dream {CURRENT} is the latest release");
        return Ok(());
    }
    if !ready {
        return Err(format!(
            "lucid-dream {latest} {NOT_READY}. This is still {CURRENT}; nothing was changed"
        ));
    }
    if check_only {
        println!(
            "lucid-dream {latest} is available (this is {CURRENT}); run `lucid-dream upgrade`"
        );
        return Ok(());
    }
    println!("Downloading lucid-dream {latest}");
    let exe = install(&latest)?;
    println!("Upgraded {CURRENT} to {latest} at {}", exe.display());
    println!(
        "A server or a waiting hook that is already running keeps the old version until it \
         restarts."
    );
    Ok(())
}

/// Download release `latest`, check it, and put it where this program is.
/// Nothing is changed unless the download matches its checksum and runs.
fn install(latest: &str) -> Result<PathBuf, String> {
    let asset = asset().ok_or(format!(
        "no release binary for this system; build it with: cargo install --git \
         https://github.com/{REPO}"
    ))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let base = format!("https://github.com/{REPO}/releases/download/v{latest}");
    let binary = get(&format!("{base}/{asset}"))?;
    let sums = String::from_utf8(get(&format!("{base}/SHA256SUMS"))?)
        .map_err(|_| "the checksum file is not text".to_string())?;
    let want = listed_sum(&sums, asset).ok_or("the release lists no checksum for this file")?;
    if !want.eq_ignore_ascii_case(&sha256_hex(&binary)) {
        return Err("checksum does not match; nothing was changed".into());
    }

    let io = |e: std::io::Error| format!("{}: {e}", exe.display());
    // Windows only runs a file whose name ends in `.exe`.
    let new = exe.with_extension(if cfg!(windows) { "new.exe" } else { "new" });
    fs::write(&new, &binary).map_err(io)?;
    let installed = runs_as(&new, latest).and_then(|()| swap(&exe, &new).map_err(io));
    if let Err(e) = installed {
        let _ = fs::remove_file(&new);
        return Err(e);
    }
    Ok(exe)
}

/// A download that cannot start, or is not the version it claims to be, is
/// never put in place: an upgrade nobody watches must not leave a machine
/// with a program that does not run.
fn runs_as(new: &Path, version: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(new, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let out = Command::new(new)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("the downloaded program does not start ({e}); nothing was changed"))?;
    let said = String::from_utf8_lossy(&out.stdout);
    if out.status.success() && said.split_whitespace().any(|word| word == version) {
        Ok(())
    } else {
        Err(format!(
            "the downloaded program did not report version {version}; nothing was changed"
        ))
    }
}

/// Upgrade without a word unless something changed: `Ok(Some(version))` when
/// a newer release was installed, `Ok(None)` when there is nothing to do yet.
pub fn quiet() -> Result<Option<String>, String> {
    let release = latest()?;
    if !newer(&release.version, CURRENT) || !release.ready || asset().is_none() {
        return Ok(None);
    }
    install(&release.version)?;
    Ok(Some(release.version))
}

// ---- Automatic upgrades: off unless the user turns them on for a machine ----

/// Set by `serve --auto-upgrade`; the same as `upgrade --auto on`, for one run.
pub const AUTO_ENV: &str = "LUCID_AUTO_UPGRADE";
/// Tells a server it was started by its own upgrade, so the ports it wants
/// may take a moment to come free.
pub const RESTARTED_ENV: &str = "LUCID_RESTARTED";
/// A client looks for a new release at most this often.
const CHECK_EVERY_SECS: i64 = 20 * 3600;

fn auto_dir() -> Result<PathBuf, String> {
    crate::hooks::state_dir("upgrade")
}

/// Turn automatic upgrades on or off for this machine.
pub fn set_auto(on: bool) -> Result<(), String> {
    let marker = auto_dir()?.join("on");
    if on {
        fs::write(&marker, "").map_err(|e| e.to_string())
    } else {
        match fs::remove_file(&marker) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

pub fn auto_on() -> bool {
    std::env::var_os(AUTO_ENV).is_some_and(|v| v == "1")
        || auto_dir().is_ok_and(|dir| dir.join("on").exists())
}

/// What the last automatic upgrade on this machine did, if one has run.
pub fn last_auto() -> Option<String> {
    fs::read_to_string(auto_dir().ok()?.join("last"))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn note_auto(what: &str) {
    if let Ok(dir) = auto_dir() {
        let _ = fs::write(
            dir.join("last"),
            format!("{} {what}\n", crate::util::Utc::now().iso()),
        );
    }
}

/// True when a client last looked for a release long enough ago to look again.
fn check_due(last: Option<i64>, now: i64) -> bool {
    last.is_none_or(|last| now - last >= CHECK_EVERY_SECS || last > now)
}

/// Called at the start of a session. When automatic upgrades are on and this
/// machine has not looked today, start `upgrade --quiet` in the background
/// and return at once: the session never waits, and the next one gets the
/// new version.
pub fn from_hook() {
    if !auto_on() || std::env::var_os(crate::dream::DREAMING_ENV).is_some() {
        return;
    }
    let Ok(dir) = auto_dir() else { return };
    let stamp = dir.join("checked");
    let last = fs::read_to_string(&stamp)
        .ok()
        .and_then(|text| text.trim().parse().ok());
    let now = crate::util::unix_now();
    if !check_due(last, now) {
        return;
    }
    // Stamp first, so sessions starting together do not all upgrade at once.
    if fs::write(&stamp, now.to_string()).is_err() {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe)
            .args(["upgrade", "--quiet"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

/// `upgrade --quiet`: what the hook starts. Says nothing; leaves a note.
pub fn run_quiet() -> Result<(), String> {
    match quiet() {
        Ok(Some(version)) => note_auto(&format!("upgraded {CURRENT} to {version}")),
        Ok(None) => {}
        Err(e) => {
            note_auto(&format!("could not upgrade: {e}"));
            return Err(e);
        }
    }
    Ok(())
}

/// The time of day a server looks for a release: half an hour after its
/// nightly dream, or 04:00 when it has none. Nobody is using it then.
pub fn server_time(dream_at: Option<(u32, u32)>) -> (u32, u32) {
    match dream_at {
        Some((h, m)) => {
            let minutes = (h * 60 + m + 30) % (24 * 60);
            (minutes / 60, minutes % 60)
        }
        None => (4, 0),
    }
}

/// For as long as the server runs: once a day at `at`, if automatic upgrades
/// are on and no dream is running, install a newer release and start it in
/// this server's place. The switch is read each time, so turning it on or
/// off needs no restart.
pub fn watch(root: PathBuf, at: (u32, u32)) {
    use chrono::{Local, TimeZone};
    std::thread::spawn(move || {
        let mut looked: Option<i64> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
            let now = Local::now();
            let Some(todays_time) = now
                .date_naive()
                .and_hms_opt(at.0, at.1, 0)
                .and_then(|t| Local.from_local_datetime(&t).earliest())
                .map(|t| t.timestamp())
            else {
                continue;
            };
            let due = now.timestamp() >= todays_time && looked.is_none_or(|l| l < todays_time);
            if !due || !auto_on() || crate::dream::running_since(&root).is_some() {
                continue;
            }
            looked = Some(now.timestamp());
            match quiet() {
                Ok(None) => {}
                Ok(Some(version)) => {
                    eprintln!("lucid-dream: upgraded {CURRENT} to {version}; restarting");
                    note_auto(&format!(
                        "upgraded {CURRENT} to {version} and restarted the server"
                    ));
                    // Only returns if the new program could not be started:
                    // this one then keeps serving, on the old version.
                    let e = restart();
                    eprintln!("lucid-dream: could not restart after the upgrade: {e}");
                    note_auto(&format!(
                        "upgraded to {version} but could not restart the server: {e}"
                    ));
                }
                Err(e) => {
                    eprintln!("lucid-dream: automatic upgrade failed: {e}");
                    note_auto(&format!("could not upgrade: {e}"));
                }
            }
        }
    });
}

/// Start the program now on disk with this run's own arguments, in this
/// process's place. Returns only on failure.
fn restart() -> String {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return e.to_string(),
    };
    let mut command = Command::new(exe);
    command
        .args(std::env::args_os().skip(1))
        .env(RESTARTED_ENV, "1");
    #[cfg(unix)]
    {
        // The same process becomes the new program, so whatever keeps the
        // server running (launchd, systemd, a terminal) sees no change.
        use std::os::unix::process::CommandExt;
        command.exec().to_string()
    }
    #[cfg(not(unix))]
    {
        // Windows cannot replace a running process: start the new one apart
        // from this one's console, then leave so it can take the ports.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        match command.spawn() {
            Ok(_) => std::process::exit(0),
            Err(e) => e.to_string(),
        }
    }
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
    fn a_release_is_ready_once_its_files_are_listed() {
        let building = serde_json::json!({ "assets": [{ "name": "SHA256SUMS" }] });
        let built = serde_json::json!({ "assets": [
            { "name": "lucid-dream-linux-x64" }, { "name": "SHA256SUMS" } ] });
        assert!(!has_files(&building, "lucid-dream-linux-x64"));
        assert!(!has_files(&serde_json::json!({}), "lucid-dream-linux-x64"));
        assert!(has_files(&built, "lucid-dream-linux-x64"));
        assert!(!has_files(&built, "lucid-dream-windows-x64.exe"));
    }

    #[test]
    fn a_server_looks_for_a_release_after_its_dream() {
        assert_eq!(server_time(Some((3, 0))), (3, 30));
        assert_eq!(server_time(Some((23, 45))), (0, 15));
        assert_eq!(server_time(None), (4, 0));
    }

    #[test]
    fn a_client_looks_about_once_a_day() {
        let now = 1_800_000_000;
        assert!(check_due(None, now));
        assert!(!check_due(Some(now - 3600), now));
        assert!(check_due(Some(now - 21 * 3600), now));
        // A clock that went backwards must not stop upgrades for good.
        assert!(check_due(Some(now + 10), now));
    }

    #[test]
    fn a_download_must_start_and_be_the_version_it_claims() {
        let me = std::env::current_exe().unwrap();
        // The test program is not lucid-dream: it does not report the version.
        assert!(runs_as(&me, "9.9.9").is_err());
        let missing = me.with_file_name("no-such-program");
        assert!(runs_as(&missing, CURRENT).is_err());
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
