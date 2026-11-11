//! lucid-dream: one shared memory that every agent reads and dream tidies.

mod client;
mod dream;
mod hooks;
mod http;
mod import;
mod init;
mod mcp;
mod store;
mod tokens;
mod util;

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
  lucid-dream init <dir>                        create a memory store (folders + git)
  lucid-dream token add <agent> [--label L] --root <dir>     issue a token (shown once)
  lucid-dream token remove <agent> [--label L] --root <dir>  revoke a token
  lucid-dream token list --root <dir>           list tokens as agent/label
  lucid-dream serve --root <dir> [--bind ADDR]  serve MCP over HTTP at /mcp
  lucid-dream serve --root <dir> --bind ADDR --only <agent>/<label>
                                                a door: accepts that one token only
  lucid-dream serve --root <dir> --stdio [--as <agent>]
  lucid-dream dream --root <dir> [--dry-run]    consolidate the inbox and commit

On any device:
  lucid-dream hook session-start [--url URL]    print the memory index (Claude Code hook)
  lucid-dream hook stop [--url URL]             send a session summary (Claude Code hook)
  lucid-dream import <path>... [--said-by WHO] (--root <dir> | --url URL)

Environment:
  LUCID_ROOT       memory store, instead of --root
  LUCID_BIND       listen address, instead of --bind (default 127.0.0.1:7878)
  LUCID_URL        server endpoint for hooks and import, e.g. http://my-pc:7878/mcp
  LUCID_TOKEN      this device's agent token, for hooks and import
  LUCID_DREAM_CMD  agent command for dream; the prompt is appended (default: claude -p)

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
        "token" => cmd_token(raw),
        "serve" => cmd_serve(raw),
        "dream" => cmd_dream(raw),
        "import" => cmd_import(raw),
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

fn cmd_serve(raw: Vec<String>) -> Result<(), String> {
    let args = Args::parse(raw, &["--root", "--bind", "--as", "--only"], &["--stdio"])?;
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
    http::serve(
        addr,
        http::App {
            store,
            tokens,
            loopback: addr.ip().is_loopback(),
            only,
        },
    )
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
    let args = Args::parse(raw, &["--root"], &["--dry-run"])?;
    args.store()?;
    // Hand git and the agent the path as given: a canonical Windows path
    // (\\?\C:\...) is one neither of them accepts as a working directory.
    dream::run(&args.root()?, args.switches.contains("--dry-run"))
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

fn hook(raw: Vec<String>) -> Result<(), String> {
    // Inside a dream run: do not inject the index or log the dream as a session.
    if std::env::var_os(dream::DREAMING_ENV).is_some() {
        return Ok(());
    }
    let args = Args::parse(raw, &["--url"], &[])?;
    let client = Client::from_env(args.option("--url"), HOOK_TIMEOUT)?;
    match args.words.first().map(String::as_str) {
        Some("session-start") => hooks::session_start(&client),
        Some("stop") => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            hooks::stop(&client, &input)
        }
        _ => Err("usage: lucid-dream hook session-start | stop".into()),
    }
}
