//! A watchdog for a machine that is not the one it watches. The PC that holds
//! the memory also runs its own health checks, so a powered-off or crashed PC
//! alerts nobody. This runs on another machine (the Mac), once a minute under
//! launchd, polls the PC's `/health` (and any other address, such as n8n's
//! `/healthz`), and posts one line to Slack and/or Telegram when a check has
//! failed twice in a row, and one line when it answers again.
//!
//! Each run is one pass and keeps what it needs in a small state file, so an
//! alert goes out once per incident however long it lasts. A line that could not
//! be delivered is tried again on the next run: the state only records an alert
//! after a channel accepted it.
//!
//! The alert addresses and tokens are secrets. They come from the environment
//! or from a `KEY=VALUE` file, never from the command line, and no error text
//! printed here can contain them.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::util::valid_name;

pub const LABEL: &str = "com.lucid-dream.watchdog";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const SEND_TIMEOUT: Duration = Duration::from_secs(15);
pub const DEFAULT_FAILS: u32 = 2;
pub const INTERVAL_SECS: u32 = 60;
pub const TELEGRAM_API: &str = "https://api.telegram.org";

pub const SLACK_ENV: &str = "LUCID_ALERT_SLACK_URL";
pub const TELEGRAM_TOKEN_ENV: &str = "LUCID_ALERT_TELEGRAM_TOKEN";
pub const TELEGRAM_CHAT_ENV: &str = "LUCID_ALERT_TELEGRAM_CHAT";

/// One address to poll. It is healthy when it answers with a 2xx status.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub name: String,
    pub url: String,
}

/// `pc=http://sim:7878/health,n8n=http://sim:5678/healthz`
pub fn parse_checks(text: &str) -> Result<Vec<Check>, String> {
    let mut checks: Vec<Check> = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, url) = part
            .split_once('=')
            .ok_or(format!("a check is name=url, not {part:?}"))?;
        if !valid_name(name) {
            return Err(format!("bad check name: {name:?}"));
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(format!(
                "check {name}: the address must start with http:// or https://"
            ));
        }
        if checks.iter().any(|c| c.name == name) {
            return Err(format!("check {name} is listed twice"));
        }
        checks.push(Check {
            name: name.to_string(),
            url: url.to_string(),
        });
    }
    if checks.is_empty() {
        return Err("no checks; pass --check pc=http://<pc-name>:7878/health".into());
    }
    Ok(checks)
}

/// What the last runs saw of one check.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CheckState {
    /// Failures in a row.
    pub fails: u32,
    /// When the first of them happened (unix seconds).
    pub down_since: Option<u64>,
    /// A down line was delivered for this incident, so the next healthy answer needs an up line.
    pub alerted: bool,
}

pub type State = BTreeMap<String, CheckState>;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Down {
        name: String,
        url: String,
        detail: String,
        down_secs: u64,
    },
    Up {
        name: String,
        down_secs: u64,
    },
}

pub struct Probe {
    pub check: Check,
    pub result: Result<(), String>,
}

/// Fold one pass of results into the state and say which lines are now owed.
/// Nothing is marked as delivered here: see `delivered`.
pub fn step(state: &mut State, probes: &[Probe], now: u64, threshold: u32) -> Vec<Event> {
    state.retain(|name, _| probes.iter().any(|p| &p.check.name == name));
    let mut events = Vec::new();
    for probe in probes {
        let name = &probe.check.name;
        match &probe.result {
            Ok(()) => match state.get_mut(name) {
                Some(entry) if entry.alerted => {
                    entry.fails = 0;
                    events.push(Event::Up {
                        name: name.clone(),
                        down_secs: now.saturating_sub(entry.down_since.unwrap_or(now)),
                    });
                }
                _ => {
                    state.remove(name);
                }
            },
            Err(detail) => {
                let entry = state.entry(name.clone()).or_default();
                entry.fails += 1;
                let since = *entry.down_since.get_or_insert(now);
                if entry.fails >= threshold && !entry.alerted {
                    events.push(Event::Down {
                        name: name.clone(),
                        url: probe.check.url.clone(),
                        detail: detail.clone(),
                        down_secs: now.saturating_sub(since),
                    });
                }
            }
        }
    }
    events
}

/// Record that a channel accepted the lines for these events.
pub fn delivered(state: &mut State, events: &[Event]) {
    for event in events {
        match event {
            Event::Down { name, .. } => {
                if let Some(entry) = state.get_mut(name) {
                    entry.alerted = true;
                }
            }
            Event::Up { name, .. } => {
                state.remove(name);
            }
        }
    }
}

fn minutes(secs: u64) -> String {
    match secs / 60 {
        0 => "under a minute".to_string(),
        1 => "1 minute".to_string(),
        n => format!("{n} minutes"),
    }
}

/// One line per event, for one message.
pub fn message(events: &[Event]) -> String {
    events
        .iter()
        .map(|event| match event {
            Event::Down {
                name,
                url,
                detail,
                down_secs,
            } => format!(
                "lucid-dream watchdog: DOWN {name} ({url}): {detail}; first failed {} ago",
                minutes(*down_secs)
            ),
            Event::Up { name, down_secs } => format!(
                "lucid-dream watchdog: UP {name} answers again after {}",
                minutes(*down_secs)
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A short reason that never includes the address (which can hold a secret).
fn describe(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, _) => format!("answered {code}"),
        ureq::Error::Transport(t) => format!("no answer ({:?})", t.kind()),
    }
}

/// Poll one address.
pub fn probe(check: &Check) -> Probe {
    let agent = ureq::AgentBuilder::new().timeout(PROBE_TIMEOUT).build();
    let result = match agent.get(&check.url).call() {
        Ok(_) => Ok(()),
        Err(e) => Err(describe(e)),
    };
    Probe {
        check: check.clone(),
        result,
    }
}

/// Where alerts go. Either or both may be set.
#[derive(Debug, Clone, Default)]
pub struct Alerts {
    /// A Slack incoming-webhook address.
    pub slack: Option<String>,
    /// A Telegram bot token and the chat to write to.
    pub telegram: Option<(String, String)>,
    /// Overridable so a test can talk to a local server.
    pub telegram_api: Option<String>,
}

impl Alerts {
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Alerts, String> {
        let slack = get(SLACK_ENV);
        let telegram = match (get(TELEGRAM_TOKEN_ENV), get(TELEGRAM_CHAT_ENV)) {
            (Some(token), Some(chat)) => Some((token, chat)),
            (None, None) => None,
            _ => {
                return Err(format!(
                    "Telegram needs both {TELEGRAM_TOKEN_ENV} and {TELEGRAM_CHAT_ENV}"
                ))
            }
        };
        if slack.is_none() && telegram.is_none() {
            return Err(format!(
                "no alert channel; set {SLACK_ENV} (a Slack incoming webhook) or \
                 {TELEGRAM_TOKEN_ENV} and {TELEGRAM_CHAT_ENV} (environment or the env file)"
            ));
        }
        Ok(Alerts {
            slack,
            telegram,
            telegram_api: None,
        })
    }

    /// Send to every channel; it counts as delivered when at least one accepted it.
    pub fn send(&self, text: &str) -> Result<(), String> {
        let mut accepted = false;
        let mut errors = Vec::new();
        if let Some(url) = &self.slack {
            match post_json(url, &json!({ "text": text })) {
                Ok(()) => accepted = true,
                Err(e) => errors.push(format!("Slack: {e}")),
            }
        }
        if let Some((token, chat)) = &self.telegram {
            let api = self.telegram_api.as_deref().unwrap_or(TELEGRAM_API);
            let url = format!("{api}/bot{token}/sendMessage");
            match post_json(&url, &json!({ "chat_id": chat, "text": text })) {
                Ok(()) => accepted = true,
                Err(e) => errors.push(format!("Telegram: {e}")),
            }
        }
        match accepted {
            true => Ok(()),
            false => Err(errors.join("; ")),
        }
    }
}

fn post_json(url: &str, body: &Value) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new().timeout(SEND_TIMEOUT).build();
    match agent
        .post(url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
    {
        Ok(_) => Ok(()),
        Err(e) => Err(describe(e)),
    }
}

/// `KEY=VALUE` lines; `#` starts a comment; quotes around a value are dropped.
pub fn parse_env_file(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            Some((key.trim().to_string(), value.to_string()))
        })
        .collect()
}

pub fn load_state(path: &Path) -> State {
    let Ok(text) = fs::read_to_string(path) else {
        return State::new();
    };
    let Ok(Value::Object(entries)) = serde_json::from_str::<Value>(&text) else {
        return State::new();
    };
    entries
        .into_iter()
        .map(|(name, e)| {
            (
                name,
                CheckState {
                    fails: e["fails"].as_u64().unwrap_or(0) as u32,
                    down_since: e["down_since"].as_u64(),
                    alerted: e["alerted"].as_bool().unwrap_or(false),
                },
            )
        })
        .collect()
}

pub fn save_state(path: &Path, state: &State) -> Result<(), String> {
    let entries: serde_json::Map<String, Value> = state
        .iter()
        .map(|(name, s)| {
            (
                name.clone(),
                json!({ "fails": s.fails, "down_since": s.down_since, "alerted": s.alerted }),
            )
        })
        .collect();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(entries)).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text + "\n").map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

pub struct Config {
    pub checks: Vec<Check>,
    pub fails: u32,
    pub state_path: PathBuf,
    pub alerts: Alerts,
}

/// One pass: poll, fold, deliver what is owed, remember what was delivered.
/// Returns the lines to print (empty when nothing happened and everything is healthy).
pub fn run_once(config: &Config, now: u64) -> Result<Vec<String>, String> {
    let probes: Vec<Probe> = config.checks.iter().map(probe).collect();
    let mut state = load_state(&config.state_path);
    let events = step(&mut state, &probes, now, config.fails);
    let mut lines = Vec::new();
    for p in &probes {
        if let Err(why) = &p.result {
            lines.push(format!("{} failing: {why}", p.check.name));
        }
    }
    if !events.is_empty() {
        let text = message(&events);
        match config.alerts.send(&text) {
            Ok(()) => {
                delivered(&mut state, &events);
                lines.push(format!("alert sent:\n{text}"));
            }
            Err(e) => lines.push(format!("alert NOT sent, will retry: {e}")),
        }
    }
    save_state(&config.state_path, &state)?;
    Ok(lines)
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The launchd agent that runs `<bin> watchdog <args>` every minute at login and after.
pub fn launchd_plist(bin: &str, args: &[String], log: &str) -> String {
    let mut program = format!(
        "    <string>{}</string>\n    <string>watchdog</string>\n",
        xml(bin)
    );
    for arg in args {
        program.push_str(&format!("    <string>{}</string>\n", xml(arg)));
    }
    let log = xml(log);
    let lines = [
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(),
        "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">".to_string(),
        "<plist version=\"1.0\">".to_string(),
        "<dict>".to_string(),
        "  <key>Label</key>".to_string(),
        format!("  <string>{LABEL}</string>"),
        "  <key>ProgramArguments</key>".to_string(),
        "  <array>".to_string(),
        program.trim_end().to_string(),
        "  </array>".to_string(),
        "  <key>StartInterval</key>".to_string(),
        format!("  <integer>{INTERVAL_SECS}</integer>"),
        "  <key>RunAtLoad</key>".to_string(),
        "  <true/>".to_string(),
        "  <key>StandardOutPath</key>".to_string(),
        format!("  <string>{log}</string>"),
        "  <key>StandardErrorPath</key>".to_string(),
        format!("  <string>{log}</string>"),
        "</dict>".to_string(),
        "</plist>".to_string(),
    ];
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::sync::{Arc, Mutex};

    /// A local HTTP server whose status can be changed, recording each request.
    struct Server {
        url: String,
        status: Arc<AtomicU16>,
        hits: Arc<Mutex<Vec<String>>>,
    }

    fn server(status: u16) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let status = Arc::new(AtomicU16::new(status));
        let hits = Arc::new(Mutex::new(Vec::new()));
        let (s, h) = (status.clone(), hits.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut raw = Vec::new();
                let mut buf = [0u8; 1024];
                loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw).to_string();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let want = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if body.len() >= want {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                h.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&raw).to_string());
                let code = s.load(Ordering::SeqCst);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {code} X\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                );
            }
        });
        Server { url, status, hits }
    }

    fn check(name: &str, url: &str) -> Check {
        Check {
            name: name.into(),
            url: url.into(),
        }
    }

    fn probes(results: &[(&str, bool)]) -> Vec<Probe> {
        results
            .iter()
            .map(|(name, ok)| Probe {
                check: check(name, &format!("http://{name}/health")),
                result: if *ok {
                    Ok(())
                } else {
                    Err("no answer (Dns)".into())
                },
            })
            .collect()
    }

    #[test]
    fn checks_parse_and_reject_the_malformed() {
        let c = parse_checks("pc=http://sim:7878/health, n8n=http://sim:5678/healthz").unwrap();
        assert_eq!(
            c,
            vec![
                check("pc", "http://sim:7878/health"),
                check("n8n", "http://sim:5678/healthz")
            ]
        );
        for bad in [
            "",
            "pc",
            "pc=ftp://x",
            "Bad Name=http://x",
            "a=http://x,a=http://y",
        ] {
            assert!(parse_checks(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn one_down_line_after_the_threshold_and_none_while_it_lasts() {
        let mut state = State::new();
        // A single failed poll is a blip.
        assert!(step(&mut state, &probes(&[("pc", false)]), 1000, 2).is_empty());
        assert!(step(&mut state, &probes(&[("pc", true)]), 1060, 2).is_empty());
        assert!(state.is_empty(), "a blip leaves nothing behind");

        assert!(step(&mut state, &probes(&[("pc", false)]), 2000, 2).is_empty());
        let events = step(&mut state, &probes(&[("pc", false)]), 2060, 2);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], Event::Down { name, down_secs: 60, .. } if name == "pc"));
        delivered(&mut state, &events);
        for t in 3..8 {
            assert!(step(&mut state, &probes(&[("pc", false)]), 2000 + t * 60, 2).is_empty());
        }
    }

    #[test]
    fn a_down_line_that_was_not_delivered_is_owed_again() {
        let mut state = State::new();
        step(&mut state, &probes(&[("pc", false)]), 0, 2);
        let first = step(&mut state, &probes(&[("pc", false)]), 60, 2);
        assert_eq!(first.len(), 1);
        // The channel refused it: nothing was recorded.
        let again = step(&mut state, &probes(&[("pc", false)]), 120, 2);
        assert_eq!(again.len(), 1);
        assert!(matches!(&again[0], Event::Down { down_secs: 120, .. }));
    }

    #[test]
    fn one_up_line_per_incident_and_none_for_a_check_that_never_alerted() {
        let mut state = State::new();
        step(&mut state, &probes(&[("pc", false), ("n8n", false)]), 0, 2);
        let events = step(&mut state, &probes(&[("pc", false), ("n8n", true)]), 60, 2);
        assert_eq!(events.len(), 1, "only pc crossed the threshold");
        delivered(&mut state, &events);
        assert!(!state.contains_key("n8n"));

        let up = step(&mut state, &probes(&[("pc", true), ("n8n", true)]), 660, 2);
        assert_eq!(
            up,
            vec![Event::Up {
                name: "pc".into(),
                down_secs: 660
            }]
        );
        // Undelivered, it is owed again; delivered, never again.
        assert_eq!(
            step(&mut state, &probes(&[("pc", true), ("n8n", true)]), 720, 2).len(),
            1
        );
        delivered(&mut state, &up);
        assert!(step(&mut state, &probes(&[("pc", true), ("n8n", true)]), 780, 2).is_empty());
        assert!(state.is_empty());
    }

    #[test]
    fn lines_say_what_and_how_long() {
        let text = message(&[
            Event::Down {
                name: "pc".into(),
                url: "http://sim:7878/health".into(),
                detail: "no answer (ConnectionFailed)".into(),
                down_secs: 125,
            },
            Event::Up {
                name: "n8n".into(),
                down_secs: 30,
            },
        ]);
        assert_eq!(
            text,
            "lucid-dream watchdog: DOWN pc (http://sim:7878/health): no answer (ConnectionFailed); first failed 2 minutes ago\n\
             lucid-dream watchdog: UP n8n answers again after under a minute"
        );
    }

    #[test]
    fn a_full_incident_against_real_servers_sends_exactly_two_lines() {
        let pc = server(200);
        let slack = server(200);
        let dir = std::env::temp_dir().join(format!("lucid-wd-{}", std::process::id()));
        let config = Config {
            checks: vec![check("pc", &format!("{}/health", pc.url))],
            fails: 2,
            state_path: dir.join("state.json"),
            alerts: Alerts {
                slack: Some(format!("{}/hooks/T0/B0/secretpart", slack.url)),
                ..Alerts::default()
            },
        };
        let sent = || slack.hits.lock().unwrap().len();

        assert!(
            run_once(&config, 0).unwrap().is_empty(),
            "healthy runs print nothing"
        );
        pc.status.store(503, Ordering::SeqCst);
        run_once(&config, 60).unwrap();
        assert_eq!(sent(), 0, "the first failure is a blip");
        let down = run_once(&config, 120).unwrap();
        assert_eq!(sent(), 1);
        assert!(down.iter().any(|l| l.contains("alert sent")));
        run_once(&config, 180).unwrap();
        run_once(&config, 240).unwrap();
        assert_eq!(sent(), 1, "one alert per incident");

        pc.status.store(200, Ordering::SeqCst);
        run_once(&config, 300).unwrap();
        assert_eq!(sent(), 2);
        run_once(&config, 360).unwrap();
        assert_eq!(sent(), 2, "one recovery per incident");

        let hits = slack.hits.lock().unwrap();
        assert!(hits[0].starts_with("POST /hooks/T0/B0/secretpart"));
        assert!(hits[0].contains("DOWN pc") && hits[0].contains("answered 503"));
        assert!(hits[1].contains("UP pc answers again after 4 minutes"));
        assert!(load_state(&config.state_path).is_empty());
    }

    #[test]
    fn an_alert_the_channel_refuses_is_retried_and_its_error_hides_the_secret() {
        let pc = server(500);
        let slack = server(500);
        let dir = std::env::temp_dir().join(format!("lucid-wd-retry-{}", std::process::id()));
        let config = Config {
            checks: vec![check("pc", &format!("{}/health", pc.url))],
            fails: 1,
            state_path: dir.join("state.json"),
            alerts: Alerts {
                slack: Some(format!("{}/hooks/secretpart", slack.url)),
                ..Alerts::default()
            },
        };
        let lines = run_once(&config, 0).unwrap().join("\n");
        assert!(
            lines.contains("alert NOT sent, will retry: Slack: answered 500"),
            "{lines}"
        );
        assert!(!lines.contains("secretpart"));
        assert!(!load_state(&config.state_path)["pc"].alerted);

        slack.status.store(200, Ordering::SeqCst);
        run_once(&config, 60).unwrap();
        assert!(load_state(&config.state_path)["pc"].alerted);
        assert_eq!(slack.hits.lock().unwrap().len(), 2);
        run_once(&config, 120).unwrap();
        assert_eq!(slack.hits.lock().unwrap().len(), 2);
    }

    #[test]
    fn telegram_posts_to_the_bot_api_and_an_unreachable_one_hides_the_token() {
        let api = server(200);
        let alerts = Alerts {
            telegram: Some(("123:SECRETTOKEN".into(), "-100555".into())),
            telegram_api: Some(api.url.clone()),
            ..Alerts::default()
        };
        alerts.send("hello").unwrap();
        let hit = api.hits.lock().unwrap()[0].clone();
        assert!(hit.starts_with("POST /bot123:SECRETTOKEN/sendMessage"));
        assert!(hit.contains(r#""chat_id":"-100555""#) && hit.contains(r#""text":"hello""#));

        let dead = Alerts {
            telegram_api: Some("http://127.0.0.1:1".into()),
            ..alerts
        };
        let error = dead.send("hello").unwrap_err();
        assert!(error.starts_with("Telegram: no answer ("), "{error}");
        assert!(!error.contains("SECRETTOKEN"));
    }

    #[test]
    fn one_working_channel_is_enough() {
        let good = server(200);
        let bad = server(500);
        let alerts = Alerts {
            slack: Some(format!("{}/x", bad.url)),
            telegram: Some(("t".into(), "c".into())),
            telegram_api: Some(good.url.clone()),
        };
        assert!(alerts.send("x").is_ok());
        assert_eq!(bad.hits.lock().unwrap().len(), 1);
    }

    #[test]
    fn alert_channels_come_from_the_lookup_and_one_is_required() {
        let from = |pairs: &'static [(&'static str, &'static str)]| {
            Alerts::from_lookup(move |k| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == k)
                    .map(|(_, v)| v.to_string())
            })
        };
        assert!(from(&[]).unwrap_err().contains(SLACK_ENV));
        assert!(from(&[(TELEGRAM_TOKEN_ENV, "t")])
            .unwrap_err()
            .contains("both"));
        assert_eq!(
            from(&[(SLACK_ENV, "https://hooks.example/x")])
                .unwrap()
                .slack
                .as_deref(),
            Some("https://hooks.example/x")
        );
        assert_eq!(
            from(&[(TELEGRAM_TOKEN_ENV, "t"), (TELEGRAM_CHAT_ENV, "c")])
                .unwrap()
                .telegram,
            Some(("t".into(), "c".into()))
        );
    }

    #[test]
    fn the_env_file_is_keys_values_comments_and_quotes() {
        let env = parse_env_file("# alerts\nLUCID_ALERT_SLACK_URL=\"https://hooks.example/a=b\"\n\n  K2 = 'v2' \nnot a pair\nK3=plain\n");
        assert_eq!(env["LUCID_ALERT_SLACK_URL"], "https://hooks.example/a=b");
        assert_eq!(env["K2"], "v2");
        assert_eq!(env["K3"], "plain");
        assert_eq!(env.len(), 3);
    }

    #[test]
    fn the_launchd_plist_runs_the_binary_every_minute_and_escapes_what_it_embeds() {
        let plist = launchd_plist(
            "/Users/jk/.local/bin/lucid-dream",
            &[
                "--check".into(),
                "pc=http://sim:7878/health?a=1&b=<2>".into(),
            ],
            "/Users/jk/Library/Logs/lucid-dream-watchdog.log",
        );
        assert!(plist.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(plist
            .contains("<dict>\n  <key>Label</key>\n  <string>com.lucid-dream.watchdog</string>"));
        assert!(plist.contains("<string>/Users/jk/.local/bin/lucid-dream</string>\n    <string>watchdog</string>\n    <string>--check</string>"));
        assert!(plist.contains("health?a=1&amp;b=&lt;2&gt;"));
        assert!(plist.contains("<key>StartInterval</key>\n  <integer>60</integer>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(plist.contains("<key>StandardErrorPath</key>\n  <string>/Users/jk/Library/Logs/lucid-dream-watchdog.log</string>"));
        assert!(plist.trim_end().ends_with("</plist>"));
        assert!(
            !plist.contains("SECRET"),
            "a plist never carries a secret: alerts are read from the env file"
        );
    }

    #[test]
    fn state_survives_a_round_trip_and_a_missing_or_broken_file_is_empty() {
        let dir = std::env::temp_dir().join(format!("lucid-wd-state-{}", std::process::id()));
        let path = dir.join("nested").join("state.json");
        assert!(load_state(&path).is_empty());
        let mut state = State::new();
        state.insert(
            "pc".into(),
            CheckState {
                fails: 3,
                down_since: Some(42),
                alerted: true,
            },
        );
        save_state(&path, &state).unwrap();
        assert_eq!(load_state(&path), state);
        fs::write(&path, "{ not json").unwrap();
        assert!(load_state(&path).is_empty());
    }
}
