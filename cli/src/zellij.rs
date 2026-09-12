//! Talking to the running zellij session over its own client library.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use zellij_client::os_input_output::{get_cli_client_os_input, ClientOsApi, ClientOsInputOutput};
use zellij_utils::consts::ZELLIJ_SOCK_DIR;
use zellij_utils::data::PaneId;
use zellij_utils::input::actions::Action;
use zellij_utils::ipc::{ClientToServerMsg, ExitReason, ServerToClientMsg};
use zellij_utils::sessions::{get_active_session, session_exists, ActiveSession};

/// One pane as reported by `list-panes` in JSON form. Unknown fields are ignored so
/// zellij can add more without breaking us.
#[derive(Debug, Clone, Deserialize)]
pub struct Pane {
    pub id: u32,
    pub is_plugin: bool,
    #[serde(default)]
    pub is_focused: bool,
    #[serde(default)]
    pub exited: bool,
    pub tab_id: usize,
    pub tab_position: usize,
    pub tab_name: String,
    #[serde(default)]
    pub pane_command: Option<String>,
}

impl Pane {
    pub fn is_terminal(&self) -> bool {
        !self.is_plugin
    }
    /// The pane's foreground command is `claude`, by basename, so `/usr/bin/claude
    /// --flags` counts too.
    pub fn runs_claude(&self) -> bool {
        self.pane_command
            .as_deref()
            .and_then(|c| c.split_whitespace().next())
            .and_then(|first| std::path::Path::new(first).file_name())
            .map_or(false, |base| base == "claude")
    }
}

pub struct Session {
    pub name: String,
    os: ClientOsInputOutput,
    terminal_id: Option<u32>,
}

/// The session named by `ZELLIJ_SESSION_NAME`, else the only running one.
pub fn resolve_session_name() -> Result<String> {
    if let Ok(name) = zellij_utils::envs::get_session_name() {
        if session_exists(&name).unwrap_or(false) {
            return Ok(name);
        }
    }
    match get_active_session() {
        ActiveSession::One(name) => Ok(name),
        ActiveSession::None => bail!("no running zellij session"),
        ActiveSession::Many => bail!("several zellij sessions are running; set ZELLIJ_SESSION_NAME"),
    }
}

impl Session {
    pub fn connect() -> Result<Self> {
        let name = resolve_session_name()?;
        let socket = ZELLIJ_SOCK_DIR.join(&name);
        if !socket.exists() {
            // connect_to_server retries forever, so check first.
            bail!("session socket not found: {}", socket.display());
        }
        let os = get_cli_client_os_input().context("setting up the zellij client")?;
        os.connect_to_server(&socket);
        let terminal_id = std::env::var("ZELLIJ_PANE_ID")
            .ok()
            .and_then(|v| v.trim().parse().ok());
        Ok(Self { name, os, terminal_id })
    }

    /// The pane this process runs in, when inside zellij.
    pub fn own_pane_id(&self) -> Option<u32> {
        self.terminal_id
    }

    /// Send one action as a CLI client and return whatever the server printed back.
    pub fn action(&self, action: Action) -> Result<Vec<String>> {
        self.os.send_to_server(ClientToServerMsg::Action {
            action,
            terminal_id: self.terminal_id,
            client_id: None,
            is_cli_client: true,
        });
        loop {
            match self.os.recv_from_server() {
                Some((ServerToClientMsg::UnblockInputThread, _)) => return Ok(vec![]),
                Some((ServerToClientMsg::Log { lines }, _)) => return Ok(lines),
                Some((ServerToClientMsg::LogError { lines }, _)) => bail!("{}", lines.join("\n")),
                Some((ServerToClientMsg::Exit { exit_reason }, _)) => match exit_reason {
                    ExitReason::Error(e) => bail!("{e}"),
                    _ => return Ok(vec![]),
                },
                Some(_) => continue,
                None => bail!("connection to the zellij server closed"),
            }
        }
    }

    pub fn panes(&self) -> Result<Vec<Pane>> {
        let lines = self.action(Action::ListPanes {
            show_tab: true,
            show_command: true,
            show_state: true,
            show_geometry: false,
            show_all: false,
            output_json: true,
        })?;
        let text = lines.join("\n");
        serde_json::from_str(&text).with_context(|| format!("parsing list-panes output:\n{text}"))
    }

    pub fn write_chars(&self, pane: u32, chars: String) -> Result<()> {
        self.action(Action::WriteCharsToPaneId { chars, pane_id: PaneId::Terminal(pane) })?;
        Ok(())
    }

    pub fn write_bytes(&self, pane: u32, bytes: Vec<u8>) -> Result<()> {
        self.action(Action::WriteToPaneId { bytes, pane_id: PaneId::Terminal(pane) })?;
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.os.send_to_server(ClientToServerMsg::ClientExited);
    }
}
