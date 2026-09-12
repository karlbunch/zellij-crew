//! zellij-crew: message other zellij tabs by name.
//!
//! Tabs are named by the zellij-crew naming daemon (a background plugin); this tool
//! delivers messages into a named tab, lists tabs, and prints the caller's tab name.

mod config;
mod state;
mod zellij;

use anyhow::{anyhow, bail, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use config::Config;
use state::{log_line, now, Message, State};
use zellij::{Pane, Session};

#[derive(Parser)]
#[command(name = "zellij-crew", version, about = "Message other zellij tabs by name")]
struct Cli {
    /// zellij-crew.kdl to use (default: next to zellij's config.kdl)
    #[arg(long, global = true, env = "ZELLIJ_CREW_CONFIG", value_name = "FILE")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Deliver a message into the named tab (case-insensitive)
    Tell {
        name: String,
        #[arg(required = true, trailing_var_arg = true)]
        message: Vec<String>,
    },
    /// List tabs with their names and recent messages
    List {
        #[arg(long)]
        json: bool,
    },
    /// Print this pane's tab name
    Name,
    /// Record this pane's status (kept for a future status indicator)
    Status { state: String },
    /// Show the effective configuration and paths
    Config,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zellij-crew: {e:#}");
            ExitCode::FAILURE
        },
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Tell { name, message } => tell(cli.config.as_deref(), &name, &message.join(" ")),
        Cmd::List { json } => list(json),
        Cmd::Name => {
            let session = Session::connect()?;
            let panes = session.panes()?;
            println!("{}", own_tab(&session, &panes)?.tab_name);
            Ok(())
        },
        Cmd::Status { state } => {
            let pane = own_pane_id().ok_or_else(|| anyhow!("not inside a zellij pane"))?;
            State::open(&zellij::resolve_session_name()?)?.set_status(pane, &state)
        },
        Cmd::Config => show_config(cli.config.as_deref()),
    }
}

fn own_pane_id() -> Option<u32> {
    std::env::var("ZELLIJ_PANE_ID").ok()?.trim().parse().ok()
}

fn own_tab<'a>(session: &Session, panes: &'a [Pane]) -> Result<&'a Pane> {
    let id = session.own_pane_id().ok_or_else(|| anyhow!("not inside a zellij pane"))?;
    panes
        .iter()
        .find(|p| p.is_terminal() && p.id == id)
        .ok_or_else(|| anyhow!("pane {id} not found in the session"))
}

fn tell(config: Option<&std::path::Path>, name: &str, message: &str) -> Result<()> {
    let cfg = Config::load(config)?;
    let session = Session::connect()?;
    let panes = session.panes()?;

    let from = own_tab(&session, &panes)
        .map(|p| p.tab_name.clone())
        .unwrap_or_else(|_| "unknown".to_owned());

    let dest: Vec<&Pane> = panes
        .iter()
        .filter(|p| p.is_terminal() && p.tab_name.eq_ignore_ascii_case(name))
        .collect();
    if dest.is_empty() {
        let mut names: Vec<&str> = panes.iter().map(|p| p.tab_name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        bail!("no tab named '{name}' (tabs: {})", names.join(", "));
    }
    let to = dest[0].tab_name.clone();
    // Prefer the pane running claude, then the tab's focused pane, then any live one.
    let pane = dest
        .iter()
        .find(|p| p.runs_claude() && !p.exited)
        .or_else(|| dest.iter().find(|p| p.is_focused && !p.exited))
        .or_else(|| dest.iter().find(|p| !p.exited))
        .ok_or_else(|| anyhow!("tab '{to}' has no live terminal pane"))?;

    let state = State::open(&session.name)?;
    let id = state.next_msg_id()?;
    let render = |t: &str| {
        t.replace("{id}", &id.to_string())
            .replace("{from}", &from)
            .replace("{to}", &to)
            .replace("{message}", message)
    };
    // Message text now, Enter after a pause, so they land as separate pty reads.
    let text = format!("\n{}{}\n{}\n", render(&cfg.prefix), message, render(&cfg.postfix));
    session.write_chars(pane.id, text)?;
    std::thread::sleep(Duration::from_millis(cfg.enter_delay_ms));
    session.write_bytes(pane.id, vec![b'\r'])?;

    let m = Message { id, ts: now(), from, to: to.clone(), pane: pane.id, msg: message.to_owned() };
    state.log_message(&m)?;
    log_line(&format!("[{}] msg#{id} {} -> {} pane {}", session.name, m.from, to, pane.id));
    println!("msg#{id} sent to {to} on pane {}", pane.id);
    Ok(())
}

#[derive(serde::Serialize)]
struct TabRow {
    tab_id: usize,
    position: usize,
    name: String,
    panes: usize,
    status: Option<String>,
    last_msg_to: Option<u64>,
    last_msg_from: Option<u64>,
}

fn list(json: bool) -> Result<()> {
    let session = Session::connect()?;
    let panes = session.panes()?;
    let state = State::open(&session.name)?;
    let msgs = state.messages();

    let mut rows: Vec<TabRow> = vec![];
    for p in panes.iter().filter(|p| p.is_terminal()) {
        if let Some(row) = rows.iter_mut().find(|r| r.tab_id == p.tab_id) {
            row.panes += 1;
            if row.status.is_none() {
                row.status = state.status(p.id).map(|s| s.state);
            }
            continue;
        }
        let last = |pick: fn(&Message) -> &str| {
            msgs.iter().rev().find(|m| pick(m).eq_ignore_ascii_case(&p.tab_name)).map(|m| m.id)
        };
        rows.push(TabRow {
            tab_id: p.tab_id,
            position: p.tab_position,
            name: p.tab_name.clone(),
            panes: 1,
            status: state.status(p.id).map(|s| s.state),
            last_msg_to: last(|m| &m.to),
            last_msg_from: last(|m| &m.from),
        });
    }
    rows.sort_by_key(|r| r.position);

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!("{:<4} {:<4} {:<16} {:>5}  {:<10} {:>7} {:>9}", "ID", "POS", "NAME", "PANES", "STATUS", "LAST_TO", "LAST_FROM");
    for r in rows {
        let opt = |v: Option<u64>| v.map(|n| format!("#{n}")).unwrap_or_else(|| "-".into());
        println!(
            "{:<4} {:<4} {:<16} {:>5}  {:<10} {:>7} {:>9}",
            r.tab_id,
            r.position,
            r.name,
            r.panes,
            r.status.as_deref().unwrap_or("-"),
            opt(r.last_msg_to),
            opt(r.last_msg_from)
        );
    }
    Ok(())
}

fn show_config(config: Option<&std::path::Path>) -> Result<()> {
    let cfg = Config::load(config)?;
    match (&cfg.path, Config::path(config)) {
        (Some(p), _) => println!("config:         {}", p.display()),
        (None, Some(p)) => println!("config:         {} (absent, using defaults)", p.display()),
        (None, None) => println!("config:         (no config dir found, using defaults)"),
    }
    println!("session:        {}", zellij::resolve_session_name().unwrap_or_else(|e| format!("({e})")));
    println!("state dir:      {}", zellij_utils::consts::ZELLIJ_TMP_DIR.join("zellij-crew").display());
    println!("log:            {}", zellij_utils::consts::ZELLIJ_TMP_LOG_DIR.join("zellij-crew.log").display());
    println!("enter_delay_ms: {}", cfg.enter_delay_ms);
    println!("prefix:         {:?}", cfg.prefix);
    println!("postfix:        {:?}", cfg.postfix);
    Ok(())
}
