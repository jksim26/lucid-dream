//! lucid-dream: one shared memory that every agent reads and dream tidies.

mod brief;
mod channel;
mod client;
mod doctor;
mod dream;
mod hooks;
mod http;
mod import;
mod init;
mod mcp;
mod pair;
mod setup;
mod store;
mod tokens;
mod trust;
mod upgrade;
mod util;
mod webhook;

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Read, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use serde_json::Value;

use client::Client;
use store::Store;
use tokens::Tokens;

const DEFAULT_BIND: &str = "127.0.0.1:7878";
const HOOK_TIMEOUT: Duration = Duration::from_secs(3);
const IMPORT_TIMEOUT: Duration = Duration::from_secs(30);

const USAGE: &str = "\
lucid-dream: one shared memory that every agent reads and dream tidies

On the machine that holds the memory:
  lucid-dream quickstart <dir> --agents grok,chatgpt,codex
                                                create the store, issue a token per agent
                                                and print the commands to run next
  lucid-dream init <dir>                        create a memory store (folders + git)
  lucid-dream token add <agent> [--label L] --root <dir>     issue a token (shown once)
  lucid-dream pair <agent> [--label L] [--minutes N] --root <dir>
                                                print a one-time code the agent exchanges for
                                                its own token at /pair, so you never copy one
  lucid-dream brief grok|chatgpt --root <dir> [--host NAME]
                                                print a block to paste into that agent: its
                                                address and a fresh pairing code, no token
  lucid-dream token remove <agent> [--label L] --root <dir>  revoke a token
  lucid-dream token list --root <dir>           list tokens as agent/label
  lucid-dream serve --root <dir> [--bind ADDR]  serve MCP over HTTP at /mcp
  lucid-dream serve --root <dir> --doors grok=7882,chatgpt=7883
                                                also open a door per cloud agent, each on its
                                                own local port and accepting only that token
  lucid-dream serve --root <dir> --dream-at 03:00 [--dream-with \"codex exec ...\"]
                                                also tidy the memory every day at that time
                                                on this machine's clock; no scheduler needed
  lucid-dream serve --root <dir> --dream-at 03:00 --backup-to <folder>
                                                after each dream, copy the whole memory into
                                                that folder as one file (a synced or second
                                                disk folder); restore with git clone
  lucid-dream serve --root <dir> --bind ADDR --only <agent>/<label>
                                                a door by itself: accepts that one token only
  lucid-dream serve --root <dir> --stdio [--as <agent>]
  lucid-dream dream --root <dir> [--dry-run] [--backup-to <folder>]
                                                consolidate the inbox and commit
  lucid-dream webhook set <agent> --url URL --root <dir>
                                                wake that agent when it gets a message;
                                                its Authorization value is read from
                                                LUCID_WEBHOOK_AUTH, never the command line
  lucid-dream webhook test|remove <agent> --root <dir>
  lucid-dream webhook list --root <dir>
  lucid-dream trust add|remove <agent> --root <dir>
                                                name an agent your trusted messenger: its
                                                requests count as yours for routine actions
  lucid-dream trust list --root <dir>

On any device:
  lucid-dream setup --url URL [--plan]           connect Claude Code on this machine: adds
                                                the hooks and the MCP server (token from
                                                LUCID_TOKEN, or typed in); --plan only shows
  lucid-dream setup --codex --url URL [--plan]   connect the Codex CLI instead: adds the MCP
                                                server to its config and the memory rule to
                                                its AGENTS.md (token from LUCID_TOKEN_CODEX)
  lucid-dream doctor [--root <dir>] [--url URL]  check this machine's connection, hooks and
                                                version, and the store when given; changes
                                                nothing
  lucid-dream upgrade [--check]                 replace this program with the latest release
                                                (checksum verified); --check only looks
  lucid-dream uninstall [--plan]                take the hooks and the MCP server back out of
                                                Claude Code; the memory is not touched
  lucid-dream record off|on|status [<folder>]    stop (or resume) sending full session
                                                records from that project folder on this
                                                machine; summaries are still sent
  lucid-dream hook session-start [--url URL]    print the memory index (Claude Code hook)
  lucid-dream hook stop [--url URL]             send a session summary (Claude Code hook)
  lucid-dream hook wait [--url URL]             wait for a message, then wake the session
                                                (Claude Code hook with asyncRewake)
  lucid-dream hook session-end [--url URL]      take the session off the `who` list
                                                (the plugins call these with --agent
                                                claude-plugin or --agent codex)
  lucid-dream import <path>... [--said-by WHO] (--root <dir> | --url URL)
  lucid-dream who [--root <dir> | --url URL]    List the agents and their sessions
  lucid-dream channel [--url URL]               Claude Code channel: pushes new messages
                                                into a running session (research preview)

Environment:
  LUCID_ROOT       memory store, instead of --root
  LUCID_BIND       listen address, instead of --bind (default 127.0.0.1:7878)
  LUCID_URL        server endpoint for hooks and import, e.g. http://my-pc:7878/mcp
  LUCID_TOKEN      this device's agent token, for hooks and import
  LUCID_BACKUP_DIR folder for a backup after each dream, instead of --backup-to
  LUCID_DREAM_CMD  agent command for dream; the prompt is appended (default: claude -p).
                   Several, separated by ||, are tried in order until one succeeds,
                   e.g. \"claude || codex exec --sandbox workspace-write\"

lucid-dream --version prints the installed version.
";

fn main() -> ExitCode {
    let mut raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.is_empty() || raw.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if raw.iter().any(|a| a == "--version" || a == "-V") {
        println!("lucid-dream {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let command = raw.remove(0);
    // Hooks must never fail a session: report nothing and exit cleanly.
    if command == "hook" {
        if let Err(message) = hook(raw) {
            if std::env::var_os("LUCID_DEBUG").is_some() {
                eprintln!("lucid-dream hook: {message}");
            }
        }
        return ExitCode::SUCCESS;
    }
    let result = match command.as_str() {
        "init" => cmd_init(raw),
        "quickstart" => cmd_quickstart(raw),
        "token" => cmd_token(raw),
        "pair" => cmd_pair(raw),
        "brief" => cmd_brief(raw),
        "serve" => cmd_serve(raw),
        "dream" => cmd_dream(raw),
        "import" => cmd_import(raw),
        "webhook" => cmd_webhook(raw),
        "channel" => cmd_channel(raw),
        "who" => cmd_who(raw),
        "trust" => cmd_trust(raw),
        "record" => cmd_record(raw),
        "setup" => cmd_setup(raw),
        "uninstall" => cmd_uninstall(raw),
        "upgrade" => cmd_upgrade(raw),
        "doctor" => return cmd_doctor(raw),
        other => Err(format!("unknown command: {other}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("lucid-dream: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Parsed command line: positional words, `--key value` options, bare switches.
struct Args {
    words: Vec<String>,
    options: HashMap<String, String>,
    switches: HashSet<String>,
}

impl Args {
    fn parse(raw: Vec<String>, options: &[&str], switches: &[&str]) -> Result<Args, String> {
        let mut args = Args {
            words: Vec::new(),
            options: HashMap::new(),
            switches: HashSet::new(),
        };
        let mut raw = raw.into_iter();
        while let Some(arg) = raw.next() {
            if options.contains(&arg.as_str()) {
                let value = raw.next().ok_or(format!("{arg} needs a value"))?;
                args.options.insert(arg, value);
            } else if switches.contains(&arg.as_str()) {
                args.switches.insert(arg);
            } else if arg.starts_with("--") {
                return Err(format!("unknown option: {arg}"));
            } else {
                args.words.push(arg);
            }
        }
        Ok(args)
    }

    fn option(&self, name: &str) -> Option<String> {
        self.options.get(name).cloned()
    }

    fn root(&self) -> Result<PathBuf, String> {
        self.option("--root")
            .or_else(|| env("LUCID_ROOT"))
            .map(PathBuf::from)
            .ok_or_else(|| "no memory store; pass --root or set LUCID_ROOT".to_string())
    }

    fn store(&self) -> Result<Store, String> {
        let root = self.root()?;
        Store::open(&root).map_err(|e| format!("cannot open memory store {}: {e}", root.display()))
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn cmd_init(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &[], &[])?;
    let [dir] = args.words.as_slice() else {
        return Err("usage: lucid-dream init <dir>".into());
    };
    init::run(std::path::Path::new(dir))?;
    println!("memory store ready at {dir}");
    Ok(())
}

fn cmd_token(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--root", "--label"], &[])?;
    let file = tokens::path(args.store()?.root());
    let label = args
        .option("--label")
        .unwrap_or_else(|| tokens::DEFAULT_LABEL.to_string());
    let words: Vec<&str> = args.words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["add", agent] => {
            let token = Tokens::add(&file, agent, &label)?;
            eprintln!("token {agent}/{label} (shown once; give it only to that client):");
            println!("{token}");
            Ok(())
        }
        ["remove", agent] => {
            if Tokens::remove(&file, agent, &label)? {
                println!("revoked {agent}/{label}");
                Ok(())
            } else {
                Err(format!("no token {agent}/{label}"))
            }
        }
        ["list"] => {
            for (agent, label) in Tokens::load(&file).map_err(|e| e.to_string())?.entries() {
                println!("{agent}/{label}");
            }
            Ok(())
        }
        _ => Err("usage: lucid-dream token add|remove <agent> [--label L] | token list".into()),
    }
}

/// Make a one-time code that an agent exchanges for its own token.
fn cmd_pair(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--root", "--label", "--minutes"], &[])?;
    let store = args.store()?;
    let [agent] = args.words.as_slice() else {
        return Err(
            "usage: lucid-dream pair <agent> [--label L] [--minutes N] --root <dir>".into(),
        );
    };
    let label = args
        .option("--label")
        .unwrap_or_else(|| tokens::DEFAULT_LABEL.to_string());
    let minutes = match args.option("--minutes") {
        Some(m) => m.parse::<i64>().map_err(|_| "bad --minutes".to_string())?,
        None => pair::DEFAULT_MINUTES,
    };
    let code = pair::create(store.root(), agent, &label, minutes)?;
    eprintln!(
        "pairing code for {agent}/{label} (works once, for {minutes} minutes; it replaces that \
         agent's current token when used):"
    );
    println!("{code}");
    eprintln!(
        "The agent gets its token with a GET of <its address>/pair/{code}, or a POST of \
         {{\"code\": \"{code}\"}} to <its address>/pair."
    );
    Ok(())
}

/// The public address of an agent's door: `https://<host>:<port>`.
fn door_address(agent: &str, host: Option<String>) -> Result<String, String> {
    let (_, _, public) = DOORS
        .iter()
        .find(|(name, _, _)| *name == agent)
        .ok_or("briefs exist for grok and chatgpt; connect other agents with `setup`")?;
    let host = host.or_else(brief::tailscale_host).ok_or(
        "cannot tell this machine's public name; pass --host <name>, e.g. \
         my-pc.my-tailnet.ts.net",
    )?;
    Ok(format!("https://{host}:{public}"))
}

/// Print what to paste into a cloud agent so it connects itself.
fn cmd_brief(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--root", "--host", "--minutes"], &[])?;
    let store = args.store()?;
    let [agent] = args.words.as_slice() else {
        return Err("usage: lucid-dream brief grok|chatgpt --root <dir> [--host NAME]".into());
    };
    let address = door_address(agent, args.option("--host"))?;
    let minutes = match args.option("--minutes") {
        Some(m) => m.parse::<i64>().map_err(|_| "bad --minutes".to_string())?,
        None => pair::DEFAULT_MINUTES,
    };
    let code = pair::create(store.root(), agent, tokens::DEFAULT_LABEL, minutes)?;
    let text = match agent.as_str() {
        "grok" => brief::grok(&address, &code),
        _ => brief::chatgpt(&address, &code),
    };
    eprintln!(
        "Paste everything below into {agent}. Its code works once, for {minutes} minutes, and \
         replaces that agent's current token when used. The server must be running.\n"
    );
    print!("{text}");
    Ok(())
}

fn cmd_serve(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(
        raw,
        &[
            "--root",
            "--bind",
            "--as",
            "--only",
            "--doors",
            "--dream-at",
            "--dream-with",
            "--backup-to",
        ],
        &["--stdio"],
    )?;
    let store = args.store()?;
    if args.switches.contains("--stdio") {
        let who = args
            .option("--as")
            .unwrap_or_else(|| http::LOCAL.to_string());
        if !util::valid_name(&who) || who == "user" {
            return Err(format!("bad agent name for --as: {who}"));
        }
        return serve_stdio(&store, &who).map_err(|e| format!("stdio: {e}"));
    }
    let bind = args
        .option("--bind")
        .or_else(|| env("LUCID_BIND"))
        .unwrap_or_else(|| DEFAULT_BIND.into());
    let addr: SocketAddr = bind
        .parse()
        .map_err(|_| format!("bad bind address: {bind}"))?;
    // `--only claude/phone` makes this listener a door for that one token.
    let only = args
        .option("--only")
        .map(|only| match only.split_once('/') {
            Some((agent, label)) => (agent.to_string(), label.to_string()),
            None => (only, tokens::DEFAULT_LABEL.to_string()),
        });
    let tokens = tokens::path(store.root());
    let root = store.root().to_path_buf();
    let mut listeners = vec![(
        addr,
        http::App {
            store,
            tokens: tokens.clone(),
            loopback: addr.ip().is_loopback(),
            only,
        },
    )];
    // `--doors grok=7882,chatgpt=7883`: a door per cloud client, in this same
    // process, each on its own loopback port for a tunnel to point at.
    for (agent, label, port) in doors(&args.option("--doors").unwrap_or_default())? {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        listeners.push((
            addr,
            http::App {
                store: Store::open(&root).map_err(|e| e.to_string())?,
                tokens: tokens.clone(),
                loopback: true,
                only: Some((agent, label)),
            },
        ));
    }
    // `--dream-with "codex exec ..."`: the agent for every dream this server
    // starts, by timer or by an agent's request.
    if let Some(command) = args.option("--dream-with") {
        std::env::set_var("LUCID_DREAM_CMD", command);
    }
    // `--backup-to <folder>`: after each dream, copy the memory there.
    if let Some(folder) = args.option("--backup-to") {
        std::env::set_var(dream::BACKUP_ENV, folder);
    }
    // `--dream-at 03:00`: the server tidies the memory itself every day, so
    // no scheduler has to be set up.
    if let Some(at) = args.option("--dream-at") {
        let at = dream::time_of_day(&at)?;
        eprintln!(
            "lucid-dream: dreaming every day at {:02}:{:02}, this machine's time",
            at.0, at.1
        );
        dream::timer(root.clone(), at);
    }
    http::serve_all(listeners)
}

/// Parse `agent[/label]=port,...` into (agent, label, port).
fn doors(list: &str) -> Result<Vec<(String, String, u16)>, String> {
    list.split(',')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|door| {
            let bad = || format!("bad door `{door}`; write it as agent=port, e.g. grok=7882");
            let (who, port) = door.split_once('=').ok_or_else(bad)?;
            let port: u16 = port.trim().parse().map_err(|_| bad())?;
            let (agent, label) = match who.trim().split_once('/') {
                Some((agent, label)) => (agent, label),
                None => (who.trim(), tokens::DEFAULT_LABEL),
            };
            if !util::valid_name(agent) || port == 0 {
                return Err(bad());
            }
            Ok((agent.to_string(), label.to_string(), port))
        })
        .collect()
}

/// Ports the guides use for each cloud client's door, and the public port a
/// Tailscale Funnel gives it. Any other agent connects to the main listener.
const DOORS: [(&str, u16, u16); 2] = [("grok", 7882, 8443), ("chatgpt", 7883, 10000)];

/// Everything the machine that holds the memory needs, in one step: the
/// store, a token per agent, and the commands to run next.
fn cmd_quickstart(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--agents"], &["--tunnels"])?;
    let [dir] = args.words.as_slice() else {
        return Err("usage: lucid-dream quickstart <dir> --agents grok,chatgpt,codex".into());
    };
    let agents: Vec<String> = args
        .option("--agents")
        .ok_or("name your agents, e.g. --agents grok,chatgpt,codex")?
        .split(',')
        .map(|a| a.trim().to_lowercase())
        .filter(|a| !a.is_empty())
        .collect();
    if let Some(bad) = agents.iter().find(|a| !util::valid_name(a) || *a == "user") {
        return Err(format!("bad agent name: {bad}"));
    }
    let root = PathBuf::from(dir);
    let existed = root.join("MEMORY.md").exists();
    init::run(&root)?;
    println!(
        "Memory store: {dir} ({})",
        if existed { "already there" } else { "created" }
    );

    let file = tokens::path(&root);
    let have = Tokens::load(&file).map_err(|e| e.to_string())?;
    let cloud = |agent: &str| DOORS.iter().any(|(name, _, _)| *name == agent);
    let local: Vec<&String> = agents.iter().filter(|a| !cloud(a)).collect();
    if !local.is_empty() {
        println!("\nTokens. Each is shown once; give it only to that agent:");
    }
    for agent in local {
        if have.has(agent, tokens::DEFAULT_LABEL) {
            println!(
                "  {agent}: already has one (make another with `token add {agent} --label <name>`)"
            );
        } else {
            let token = Tokens::add(&file, agent, tokens::DEFAULT_LABEL)?;
            println!("  {agent}: {token}");
        }
    }

    let doors: Vec<&(&str, u16, u16)> = DOORS
        .iter()
        .filter(|(agent, _, _)| agents.iter().any(|a| a == agent))
        .collect();
    let mut serve = format!("lucid-dream serve --root {dir}");
    if !doors.is_empty() {
        let list: Vec<String> = doors
            .iter()
            .map(|(a, port, _)| format!("{a}={port}"))
            .collect();
        serve.push_str(&format!(" --doors {}", list.join(",")));
    }
    serve.push_str(" --dream-at 03:00");
    let has = |name: &str| agents.iter().any(|a| a == name);
    if has("codex") && !has("claude") {
        serve.push_str(" --dream-with \"codex exec --sandbox workspace-write\"");
    }
    println!(
        "\nNext, start the server and leave it running. It also tidies the memory every \
         night at 03:00:\n  {serve}"
    );
    if !doors.is_empty() {
        println!(
            "\nGrok and ChatGPT call from the internet, so each needs a public HTTPS address \
             that leads only to its own door."
        );
        let host = brief::tailscale_host();
        if args.switches.contains("--tunnels") {
            for (agent, port, public) in &doors {
                let done = std::process::Command::new("tailscale")
                    .args([
                        "funnel",
                        "--bg",
                        &format!("--https={public}"),
                        &port.to_string(),
                    ])
                    .output();
                match done {
                    Ok(out) if out.status.success() => {
                        println!("  tunnel for {agent}: open on public port {public}")
                    }
                    Ok(out) => println!(
                        "  tunnel for {agent}: Tailscale refused: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    ),
                    Err(_) => println!(
                        "  tunnel for {agent}: `tailscale` is not installed or not on the PATH"
                    ),
                }
            }
        } else {
            println!(
                "With Tailscale installed (or run this again with --tunnels to do it for you):"
            );
            for (agent, port, public) in &doors {
                println!("  tailscale funnel --bg --https={public} {port}     # {agent}");
            }
        }
        let name = host.unwrap_or_else(|| "<this-machine>.<tailnet>.ts.net".to_string());
        for (agent, _, public) in &doors {
            println!("  {agent}'s address: https://{name}:{public}/mcp");
        }
        println!(
            "\nOnce the server is running, connect each one by pasting a ready-made block \
             into it. The block lets the agent fetch its own token, so you never copy one:"
        );
        for (agent, _, _) in &doors {
            println!("  lucid-dream brief {agent} --root {dir}");
        }
    }
    println!(
        "\nGuides for each agent: https://github.com/jksim26/lucid-dream#connect-the-agents\n\
         Check everything with: lucid-dream doctor --root {dir}"
    );
    Ok(())
}

/// stdio transport: one JSON-RPC message per line.
fn serve_stdio(store: &Store, who: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => mcp::handle(store, who, &msg),
            Err(_) => Some(mcp::error(Value::Null, -32700, "parse error")),
        };
        if let Some(reply) = reply {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}

fn cmd_dream(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--root", "--backup-to"], &["--dry-run"])?;
    args.store()?;
    if let Some(folder) = args.option("--backup-to") {
        std::env::set_var(dream::BACKUP_ENV, folder);
    }
    // Hand git and the agent the path as given: a canonical Windows path
    // (\\?\C:\...) is one neither of them accepts as a working directory.
    let root = args.root()?;
    if args.switches.contains("--dry-run") {
        return dream::run(&root, true);
    }
    println!("{}", dream::run_recorded(&root, "hand")?);
    Ok(())
}

fn cmd_import(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--root", "--url", "--said-by", "--as"], &[])?;
    if args.words.is_empty() {
        return Err(
            "usage: lucid-dream import <path>... [--said-by WHO] (--root <dir> | --url URL)".into(),
        );
    }
    let said_by = args
        .option("--said-by")
        .unwrap_or_else(|| "user".to_string());
    let local = args.option("--root").is_some()
        || (args.option("--url").is_none() && env("LUCID_URL").is_none());
    let report = if local {
        let store = args.store()?;
        let source = args.option("--as").unwrap_or_else(|| "import".to_string());
        import::run(
            &args.words,
            &import::Sink::Local {
                store: &store,
                source: &source,
            },
            &said_by,
        )?
    } else {
        let client = Client::from_env(args.option("--url"), IMPORT_TIMEOUT)?;
        import::run(&args.words, &import::Sink::Remote(&client), &said_by)?
    };
    println!(
        "imported {} notes into the inbox ({} already there, {} skipped)",
        report.added, report.duplicates, report.skipped
    );
    Ok(())
}

/// How often a listener checks for messages when the server's signal does
/// not reach it.
fn poll_interval() -> Duration {
    let seconds = env("LUCID_CHANNEL_POLL")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(20);
    Duration::from_secs(seconds.max(2))
}

fn cmd_channel(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--url"], &[])?;
    let client = Client::from_env(args.option("--url"), HOOK_TIMEOUT)?;
    let project = std::env::current_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    // Claude Code tells its helpers which session they belong to; the same id
    // reaches the SessionStart hook, so both check in as one session.
    let id = env("CLAUDE_CODE_SESSION_ID")
        .unwrap_or_else(|| format!("{}-{}", std::process::id(), util::unix_now()));
    let me = serde_json::json!({
        "session": hooks::session_name(None), "id": id, "device": hooks::hostname(),
        "project": project, "live": true,
    });
    channel::run(client, poll_interval(), me).map_err(|e| format!("channel: {e}"))
}

/// The session list, for the user: from the store on the PC that holds it,
/// from the server anywhere else.
fn cmd_who(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--root", "--url"], &[])?;
    let list = if args.option("--root").is_some() || env("LUCID_ROOT").is_some() {
        mcp::who_is_around(&args.store()?, "")
    } else {
        Client::from_env(args.option("--url"), HOOK_TIMEOUT)?.call("who", serde_json::json!({}))?
    };
    println!("{}", list.trim_end());
    Ok(())
}

/// Claude Code's user settings file, or the one named with `--settings`.
fn settings_file(args: &Args) -> Result<PathBuf, String> {
    match args.option("--settings") {
        Some(path) => Ok(PathBuf::from(path)),
        None => {
            // Claude Code keeps its files elsewhere when this is set.
            if let Some(dir) = env("CLAUDE_CONFIG_DIR") {
                return Ok(PathBuf::from(dir).join("settings.json"));
            }
            let home = env("HOME")
                .or_else(|| env("USERPROFILE"))
                .ok_or("no home folder; pass --settings <file>")?;
            Ok(PathBuf::from(home).join(".claude").join("settings.json"))
        }
    }
}

/// Take lucid-dream back out of Claude Code on this machine.
fn cmd_uninstall(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--settings"], &["--plan"])?;
    setup::uninstall(&settings_file(&args)?, args.switches.contains("--plan"))
}

fn cmd_upgrade(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &[], &["--check", "--force"])?;
    upgrade::run(
        args.switches.contains("--check"),
        args.switches.contains("--force"),
    )
}

/// Check this machine's setup. Exits with failure when something is broken.
fn cmd_doctor(raw: Vec<String>) -> ExitCode {
    let checked = (|| -> Result<bool, String> {
        let args = Args::parse(raw, &["--root", "--url", "--settings"], &["--offline"])?;
        let root = args
            .option("--root")
            .or_else(|| env("LUCID_ROOT"))
            .map(PathBuf::from);
        Ok(doctor::run(&doctor::Options {
            settings: &settings_file(&args)?,
            url: args.option("--url"),
            root: root.as_deref(),
            offline: args.switches.contains("--offline"),
        }))
    })();
    match checked {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("lucid-dream: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Connect Claude Code on this machine to a memory server.
fn cmd_setup(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(
        raw,
        &["--url", "--settings", "--codex-home"],
        &["--plan", "--codex"],
    )?;
    let url = args
        .option("--url")
        .or_else(|| env("LUCID_URL"))
        .ok_or("no server; pass --url http://<pc-name>:7878/mcp")?;
    // `--codex`: the Codex CLI instead of Claude Code. Its token stays in an
    // environment variable that Codex reads; this only names the variable.
    if args.switches.contains("--codex") {
        let home = args
            .option("--codex-home")
            .map(PathBuf::from)
            .or_else(setup::codex_home)
            .ok_or("no home folder; pass --codex-home")?;
        setup::codex(&home, &url, args.switches.contains("--plan"))?;
        match env(setup::CODEX_TOKEN_ENV) {
            Some(token) => {
                let client = Client::with(url.clone(), token, HOOK_TIMEOUT);
                match client.call("who", serde_json::json!({})) {
                    Ok(_) => println!("Server: reachable at {url}, Codex's token accepted"),
                    Err(e) => println!("Server: {e}"),
                }
            }
            None => println!(
                "Next: put Codex's token (from `lucid-dream token add codex` on the memory's \
                 machine) in the environment variable {} where Codex runs.",
                setup::CODEX_TOKEN_ENV
            ),
        }
        return Ok(());
    }
    // The token never goes on the command line: from LUCID_TOKEN, or typed in.
    let token = match env("LUCID_TOKEN") {
        Some(token) => token,
        None => {
            eprint!("Token for this device (from `lucid-dream token add claude` on the PC): ");
            let mut line = String::new();
            std::io::stdin()
                .lock()
                .read_line(&mut line)
                .map_err(|e| e.to_string())?;
            line.trim().to_string()
        }
    };
    if token.is_empty() {
        return Err("no token given".into());
    }
    let settings = settings_file(&args)?;
    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .display()
        .to_string()
        // Claude Code runs hooks through a Unix-style shell on Windows too.
        .replace('\\', "/");
    let dry = args.switches.contains("--plan");
    // Check the server and the token before touching anything.
    let client = Client::with(url.clone(), token.clone(), HOOK_TIMEOUT);
    match client.call("who", serde_json::json!({})) {
        Ok(_) => println!("Server: reachable at {url}, token accepted"),
        Err(e) if dry => println!("Server: {e}"),
        Err(e) => return Err(format!("{e}\nNothing was changed.")),
    }
    setup::run(
        &setup::Plan {
            url,
            token,
            exe,
            settings,
        },
        dry,
    )
}

/// Name an agent the user's trusted messenger, or take that back. Only on
/// the machine that holds the memory: it is the user's decision, not one an
/// agent can make over the network.
/// Turn full session records off or on for a project folder, on this
/// machine. Off is a marker file in the folder; the hook that sends records
/// looks for it there and in every folder above the session's.
fn cmd_record(raw: Vec<String>) -> Result<(), String> {
    const HOW: &str = "usage: lucid-dream record off|on|status [<project folder>]";
    let args = Args::parse(raw, &[], &[])?;
    let words: Vec<&str> = args.words.iter().map(String::as_str).collect();
    let (action, dir) = match words.as_slice() {
        [action] => (*action, std::env::current_dir().map_err(|e| e.to_string())?),
        [action, dir] => (*action, PathBuf::from(dir)),
        _ => return Err(HOW.into()),
    };
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    let marker = dir.join(hooks::RECORD_OFF);
    let io = |e: std::io::Error| format!("{}: {e}", marker.display());
    match action {
        "off" => {
            std::fs::write(
                &marker,
                "lucid-dream: sessions in this folder send no full record\n",
            )
            .map_err(io)?;
            println!(
                "Records are off for {} and every folder under it ({} was added). Sessions \
                 there still send their short summary. Records already stored stay where \
                 they are.",
                dir.display(),
                hooks::RECORD_OFF
            );
        }
        "on" => match std::fs::remove_file(&marker) {
            Ok(()) => println!("Records are on again for {}", dir.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match hooks::record_off(&dir) {
                Some(above) => println!(
                    "Records are off because of {}; turn them on in that folder",
                    above.display()
                ),
                None => println!("Records were already on for {}", dir.display()),
            },
            Err(e) => return Err(io(e)),
        },
        "status" => match hooks::record_off(&dir) {
            Some(marker) => println!("off, because of {}", marker.display()),
            None => println!("on"),
        },
        _ => return Err(HOW.into()),
    }
    Ok(())
}

fn cmd_trust(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--root"], &[])?;
    let file = trust::path(args.store()?.root());
    let words: Vec<&str> = args.words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["add", agent] => {
            trust::add(&file, agent)?;
            println!(
                "{agent} is now a trusted messenger: other agents are told its requests may \
                 be treated as yours for routine, undoable actions, and it may read \
                 full session records through its door"
            );
            Ok(())
        }
        ["remove", agent] => {
            if trust::remove(&file, agent)? {
                println!("{agent} is no longer a trusted messenger");
                Ok(())
            } else {
                Err(format!("{agent} was not a trusted messenger"))
            }
        }
        ["list"] => {
            for agent in trust::load(&file) {
                println!("{agent}");
            }
            Ok(())
        }
        _ => Err("usage: lucid-dream trust add|remove <agent> | trust list".into()),
    }
}

fn cmd_webhook(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--root", "--url"], &[])?;
    let file = webhook::path(args.store()?.root());
    let words: Vec<&str> = args.words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["set", agent] => {
            let url = args.option("--url").ok_or("webhook set needs --url")?;
            let authorization = env("LUCID_WEBHOOK_AUTH");
            let with = if authorization.is_some() {
                "with"
            } else {
                "without"
            };
            webhook::set(&file, agent, &url, authorization)?;
            println!(
                "{agent} will be woken when it gets a message ({with} an Authorization value)"
            );
            Ok(())
        }
        ["test", agent] => {
            let hooks = webhook::load(&file);
            let hook = hooks.get(*agent).ok_or(format!("no webhook for {agent}"))?;
            let status = webhook::post(hook, &webhook::payload(agent, "lucid-dream-test"))?;
            println!("webhook for {agent} answered {status}");
            Ok(())
        }
        ["remove", agent] => {
            if webhook::remove(&file, agent)? {
                println!("removed the webhook for {agent}");
                Ok(())
            } else {
                Err(format!("no webhook for {agent}"))
            }
        }
        ["list"] => {
            for (agent, hook) in webhook::load(&file) {
                // The address is a secret: show only which service it points at.
                let host = hook.url.split('/').nth(2).unwrap_or("?");
                let auth = if hook.authorization.is_some() {
                    "with"
                } else {
                    "no"
                };
                println!("{agent}  {host}  ({auth} Authorization)");
            }
            Ok(())
        }
        _ => Err("usage: lucid-dream webhook set|test|remove <agent> | webhook list".into()),
    }
}

fn hook(raw: Vec<String>) -> Result<(), String> {
    // Inside a dream run: do not inject the index or log the dream as a session.
    if std::env::var_os(dream::DREAMING_ENV).is_some() {
        return Ok(());
    }
    let args = Args::parse(raw, &["--url", "--agent"], &[])?;
    let agent = args.option("--agent");
    // A plugin's hooks stand aside where `setup` already put the same hooks
    // in Claude Code's settings, so nothing is logged or woken twice.
    if agent.as_deref() == Some("claude-plugin") {
        let settings = settings_file(&args)
            .ok()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        if settings.is_some_and(|settings| setup::has_hooks(&settings)) {
            return Ok(());
        }
    }
    let client = Client::for_agent(args.option("--url"), HOOK_TIMEOUT, agent.as_deref())?;
    match args.words.first().map(String::as_str) {
        Some("session-start") => {
            // Claude Code passes details on stdin; a person running this by hand passes nothing.
            let mut input = String::new();
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                let _ = std::io::stdin().read_to_string(&mut input);
            }
            hooks::session_start(&client, &input)
        }
        Some("stop") => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            hooks::stop(&client, &input)
        }
        // Run with `asyncRewake`: waits for a message, then wakes the session.
        Some("wait") => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            hooks::wait(std::sync::Arc::new(client), &input, poll_interval())
        }
        Some("session-end") => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            hooks::session_end(&client, &input)
        }
        _ => Err("usage: lucid-dream hook session-start | stop | wait | session-end".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_doors() {
        assert_eq!(
            doors("grok=7882, claude/phone=7881,").unwrap(),
            [
                ("grok".to_string(), "default".to_string(), 7882),
                ("claude".to_string(), "phone".to_string(), 7881),
            ]
        );
        assert!(doors("").unwrap().is_empty());
        for bad in ["grok", "grok=abc", "grok=0", "=7882", "gr ok=7882"] {
            assert!(doors(bad).is_err(), "{bad}");
        }
    }
}
