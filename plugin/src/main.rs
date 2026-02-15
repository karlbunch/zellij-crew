// zellij-crew: Tab-bar plugin with named tabs and activity indicators
// Based on zellij-tab-bar-indexed (MIT license)

mod line;
mod tab;

use std::cmp::{max, min};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryInto;
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tab::get_tab_to_focus;
use zellij_tile::prelude::*;

use crate::line::tab_line;
use crate::tab::tab_style;

// ============================================================================
// LinePart - shared with line.rs and tab.rs
// ============================================================================

#[derive(Debug, Default, Clone)]
pub struct LinePart {
    pub part: String,
    pub len: usize,
    pub tab_index: Option<usize>,
}

impl LinePart {
    pub fn append(&mut self, to_append: &LinePart) {
        self.part.push_str(&to_append.part);
        self.len += to_append.len;
    }
}

pub static ARROW_SEPARATOR: &str = "\u{e0b0}";

// ============================================================================
// Configuration
// ============================================================================

const DEFAULT_NAMES: &str = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey xray yankee zulu";

// Election protocol: all tab-bar instances elect a leader among themselves.
// Tiebreaker: highest plugin_id wins (newer instances get higher IDs).
const ELECTION_TIMEOUT_SECS: f64 = 0.3;
const MSG_LEADER_PING: &str = "crew-leader-ping";
const MSG_LEADER_ACK: &str = "crew-leader-ack";
const MSG_LEADER_CLAIM: &str = "crew-leader-claim";
const MSG_LEADER_RESIGN: &str = "crew-leader-resign";

#[derive(Debug, Clone, PartialEq)]
enum AllocationMode {
    RoundRobin,
    FillIn,
}

impl Default for AllocationMode {
    fn default() -> Self {
        AllocationMode::RoundRobin
    }
}

#[derive(Debug, Clone, Default)]
struct Config {
    names: Vec<String>,
    mode: AllocationMode,
    // TODO: Implement show_position feature (see DESIGN.md Future Enhancements)
    // show_position: bool,  // Would show "alpha <1>" style names
    hide_swap_layout_indication: bool,
    /// Per-status indicator overrides. Key present with empty string = suppress brackets entirely.
    /// Key absent = use default emoji.
    status_indicators: HashMap<ActivityStatus, String>,
    /// Appended to tell messages. Substitutions: {from}, {to}, {message}, {id}.
    tell_append: String,
    /// Delay in ms between message text and Enter keystroke.
    tell_delay_ms: u32,
    /// Seconds of no terminal output before an idle tab transitions to sleeping (0 = disabled).
    idle_sleep_secs: u64,
}

impl Config {
    fn from_btreemap(config: &BTreeMap<String, String>) -> Self {
        let names_str = config
            .get("names")
            .map(|s| s.as_str())
            .unwrap_or(DEFAULT_NAMES);

        let names: Vec<String> = names_str
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let mode = match config.get("mode").map(|s| s.as_str()) {
            Some("fill-in") => AllocationMode::FillIn,
            _ => AllocationMode::RoundRobin,
        };

        let hide_swap_layout_indication = config
            .get("hide_swap_layout_indication")
            .map(|s| s == "true")
            .unwrap_or(false);

        let mut status_indicators = HashMap::new();
        let status_keys: &[(&str, ActivityStatus)] = &[
            ("status_unknown", ActivityStatus::Unknown),
            ("status_idle", ActivityStatus::Idle),
            ("status_working", ActivityStatus::Working),
            ("status_question", ActivityStatus::Question),
            ("status_sleeping", ActivityStatus::Sleeping),
            ("status_watching", ActivityStatus::Watching),
            ("status_attention", ActivityStatus::Attention),
        ];
        for (key, variant) in status_keys {
            if let Some(val) = config.get(*key) {
                status_indicators.insert(variant.clone(), val.clone());
            }
        }

        let tell_append = config
            .get("tell_append")
            .cloned()
            .unwrap_or_else(|| "*CRITICAL* Reply ONLY by running this bash command, do not just output your response: zellij-crew tell {from} \"your reply here\"".to_string());

        let tell_delay_ms = config
            .get("tell_delay_ms")
            .and_then(|s| s.parse().ok())
            .unwrap_or(250);

        let idle_sleep_secs = config
            .get("idle_sleep_secs")
            .and_then(|s| s.parse().ok())
            .unwrap_or(300);

        Config {
            names,
            mode,
            hide_swap_layout_indication,
            status_indicators,
            tell_append,
            tell_delay_ms,
            idle_sleep_secs,
        }
    }

    /// Returns the display string for a status, or None to suppress the indicator entirely.
    fn indicator_for(&self, status: &ActivityStatus) -> Option<&str> {
        match self.status_indicators.get(status) {
            Some(s) if s.is_empty() => None,
            Some(s) => Some(s.as_str()),
            None => Some(status.default_indicator()),
        }
    }
}

// ============================================================================
// Plugin State
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum ActivityStatus {
    Unknown,
    Idle,
    Working,
    Question,
    Sleeping,
    Watching,
    Attention,
}

impl ActivityStatus {
    fn default_indicator(&self) -> &'static str {
        match self {
            Self::Unknown => "🫥",
            Self::Idle => "🥱",
            Self::Working => "🤖",
            Self::Question => "🙋",
            Self::Sleeping => "😴",
            Self::Watching => "👀",
            Self::Attention => "🔔",
        }
    }

    fn status_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Question => "question",
            Self::Sleeping => "sleeping",
            Self::Watching => "watching",
            Self::Attention => "attention",
        }
    }
}

impl Default for ActivityStatus {
    fn default() -> Self {
        ActivityStatus::Unknown
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrewTabState {
    tab_id: usize,                   // Stable ID from TabInfo.tab_id (survives reordering)
    position: usize,                 // Current position (updates when tabs reorder)
    name: String,                    // Current name ("Alice" after rename, "Tab #5" before)

    // ADR: Why pending_rename flag?
    // Prevents infinite rename loops: when we call rename_tab_with_id(), the next TabUpdate
    // may still show the old name (rename not processed yet). We track pending renames
    // to avoid re-renaming the same tab. Once TabUpdate shows the new name, we confirm
    // by matching against pending_rename and clearing the flag.
    #[serde(skip)]                   // Don't serialize internal rename tracking
    pending_rename: Option<String>,  // Some("Alice") when rename sent, waiting for confirmation

    user_defined: bool,              // true if user named it, false if from our pool
    status: ActivityStatus,          // Current activity status

    #[serde(skip)]
    last_msg_to: Option<(u32, u64)>,       // (msg_id, epoch_secs) - last message sent TO this tab
    #[serde(skip)]
    last_msg_from: Option<(u32, u64)>,     // (msg_id, epoch_secs) - last message FROM this tab
    #[serde(skip)]
    status_updated_at: Option<u64>,        // epoch_secs when status last changed
    #[serde(skip)]
    last_activity_at: Option<u64>,         // epoch_secs of last PaneRenderReport for this tab
}

#[derive(Default)]
struct State {
    // Common state (all instances are tab-bar panes)
    instance_id: String,
    plugin_id: u32,           // From get_plugin_ids(), election tiebreaker (highest wins)
    mode_info: ModeInfo,
    config: Config,
    is_leader: bool,          // Determined by election protocol
    election_pending: bool,   // Waiting for election timeout

    // Leader-only state
    known_tabs: HashMap<usize, CrewTabState>,  // tab_id -> CrewTabState
    pane_manifest: Option<PaneManifest>,       // For mapping pane_id -> tab
    last_assigned_idx: Option<usize>,
    inherited_state: Option<HashMap<usize, CrewTabState>>,  // From leader resign
    pending_tell_enter: Option<u32>,  // Pane ID awaiting delayed \r after tell
    next_msg_id: u32,                 // Monotonic counter for tell message IDs

    // All instances (for rendering)
    received_tabs: Vec<CrewTabState>,  // From leader broadcast (renderers only)
    tabs: Vec<TabInfo>,                // From TabUpdate
    active_tab_idx: usize,
    tab_line: Vec<LinePart>,           // Cached for mouse clicks
}

// ============================================================================
// Helper Functions
// ============================================================================

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Rename a tab by its stable tab_id (not position).
/// Mirrors the shim pattern from zellij-tile but uses RenameTabWithId instead of RenameTab.
fn rename_tab_with_id(tab_id: usize, name: String) {
    use plugin_api::plugin_command::ProtobufPluginCommand;

    #[link(wasm_import_module = "zellij")]
    extern "C" {
        fn host_run_plugin_command();
    }

    let plugin_command = PluginCommand::RenameTabWithId(tab_id as u64, name);
    let protobuf_plugin_command: ProtobufPluginCommand = plugin_command.try_into().unwrap();
    object_to_stdout(&protobuf_plugin_command.encode_to_vec());
    unsafe { host_run_plugin_command() };
}

// ============================================================================
// Leader Election
// ============================================================================

impl State {
    fn start_election(&mut self) {
        if self.election_pending {
            return;
        }
        self.election_pending = true;
        let payload = serde_json::json!({"plugin_id": self.plugin_id});
        eprintln!("[crew:{}:plugin{}] Starting election", self.instance_id, self.plugin_id);
        pipe_message_to_plugin(
            MessageToPlugin::new(MSG_LEADER_PING)
                .with_payload(serde_json::to_string(&payload).unwrap_or_default()),
        );
        set_timeout(ELECTION_TIMEOUT_SECS);
    }

    fn become_leader(&mut self) {
        self.is_leader = true;
        self.election_pending = false;
        eprintln!("[crew:{}:plugin{}] Became LEADER", self.instance_id, self.plugin_id);

        // Adopt inherited state if present (from a resign message)
        if let Some(inherited) = self.inherited_state.take() {
            eprintln!("[crew:{}:leader] Adopting {} inherited tabs", self.instance_id, inherited.len());
            self.known_tabs = inherited;
        }

        // Broadcast claim so others know
        let payload = serde_json::json!({"plugin_id": self.plugin_id});
        pipe_message_to_plugin(
            MessageToPlugin::new(MSG_LEADER_CLAIM)
                .with_payload(serde_json::to_string(&payload).unwrap_or_default()),
        );

        // Arm periodic activity check timer (if idle_sleep_secs enabled)
        if self.config.idle_sleep_secs > 0 {
            set_timeout(30.0);
        }

        self.broadcast_state();
    }

    fn resign_leadership(&mut self) {
        if !self.is_leader {
            return;
        }
        eprintln!("[crew:{}:leader] Resigning leadership", self.instance_id);

        let state: Vec<&CrewTabState> = self.known_tabs.values().collect();
        let payload = serde_json::json!({
            "plugin_id": self.plugin_id,
            "state": state,
        });
        pipe_message_to_plugin(
            MessageToPlugin::new(MSG_LEADER_RESIGN)
                .with_payload(serde_json::to_string(&payload).unwrap_or_default()),
        );

        self.is_leader = false;
    }
}

// ============================================================================
// Leader State Management
// ============================================================================

impl State {
    fn broadcast_state(&self) {
        if !self.is_leader {
            return; // Only leader broadcasts
        }

        let tabs: Vec<&CrewTabState> = self.known_tabs.values().collect();

        if let Ok(json) = serde_json::to_string(&tabs) {
            eprintln!("[crew:{}:leader] Broadcasting state: {} tabs", self.instance_id, tabs.len());

            pipe_message_to_plugin(
                MessageToPlugin::new("crew-state")
                    .with_payload(json),
            );
        } else {
            eprintln!("[crew:{}:leader] ERROR: Failed to serialize state", self.instance_id);
        }
    }

    fn allocate_from_pool(&mut self) -> Option<String> {
        if self.config.names.is_empty() {
            return None;
        }

        let used: HashSet<String> = self
            .known_tabs
            .values()
            .filter(|t| !t.user_defined)
            .map(|t| t.name.clone())
            .collect();

        match self.config.mode {
            AllocationMode::RoundRobin => {
                let start_idx = self.last_assigned_idx.map(|i| i + 1).unwrap_or(0);
                let pool_len = self.config.names.len();

                for offset in 0..pool_len {
                    let idx = (start_idx + offset) % pool_len;
                    let name = &self.config.names[idx];
                    if !used.contains(name) {
                        self.last_assigned_idx = Some(idx);
                        return Some(name.clone());
                    }
                }
                None
            }
            AllocationMode::FillIn => {
                for name in &self.config.names {
                    if !used.contains(name) {
                        return Some(name.clone());
                    }
                }
                None
            }
        }
    }

    fn handle_leader_tab_update(&mut self, tabs: &[TabInfo]) {
        eprintln!("[crew:{}:leader] Processing {} tabs", self.instance_id, tabs.len());

        // Track which tab IDs we've seen in this update
        let mut seen_tab_ids = HashSet::new();

        for tab in tabs {
            let tab_id = tab.tab_id;
            seen_tab_ids.insert(tab_id);

            if let Some(crew_tab) = self.known_tabs.get_mut(&tab_id) {
                // Existing tab - check for pending rename confirmation or user rename
                if let Some(pending) = &crew_tab.pending_rename {
                    if pending == &tab.name {
                        eprintln!("[crew:{}:leader] Rename confirmed: tab {} -> {} (pos {})",
                            self.instance_id, tab_id, tab.name, tab.position);
                        crew_tab.name = tab.name.clone();
                        crew_tab.position = tab.position;
                        crew_tab.pending_rename = None;
                    }
                    // else: rename not yet reflected, keep waiting
                } else if crew_tab.name != tab.name {
                    // Name changed without pending rename - user renamed it
                    eprintln!("[crew:{}:leader] Tab {} renamed by user: '{}' -> '{}' (pos {})",
                        self.instance_id, tab_id, crew_tab.name, tab.name, tab.position);
                    crew_tab.name = tab.name.clone();
                    crew_tab.user_defined = true;
                    crew_tab.position = tab.position;
                }
                if crew_tab.position != tab.position {
                    eprintln!("[crew:{}:leader] Tab {} '{}' moved pos {} -> {}",
                        self.instance_id, tab_id, crew_tab.name, crew_tab.position, tab.position);
                    crew_tab.position = tab.position;
                }
            } else {
                // New tab
                if tab.name.starts_with("Tab #") {
                    // Default name - allocate from pool and rename
                    if let Some(new_name) = self.allocate_from_pool() {
                        eprintln!("[crew:{}:leader] New tab {}: renaming '{}' -> {} (pos {})",
                            self.instance_id, tab_id, tab.name, new_name, tab.position);
                        rename_tab_with_id(tab_id, new_name.clone());

                        self.known_tabs.insert(tab_id, CrewTabState {
                            tab_id,
                            position: tab.position,
                            name: tab.name.clone(),
                            pending_rename: Some(new_name),
                            user_defined: false,
                            status: ActivityStatus::Unknown,
                            last_msg_to: None,
                            last_msg_from: None,
                            status_updated_at: None,
                            last_activity_at: None,
                        });
                    } else {
                        eprintln!("[crew:{}:leader] Pool exhausted, leaving tab {} unnamed", self.instance_id, tab_id);
                    }
                } else {
                    // User-defined name on a new tab - track it
                    eprintln!("[crew:{}:leader] New tab {} with user name '{}' (pos {})",
                        self.instance_id, tab_id, tab.name, tab.position);
                    self.known_tabs.insert(tab_id, CrewTabState {
                        tab_id,
                        position: tab.position,
                        name: tab.name.clone(),
                        pending_rename: None,
                        user_defined: true,
                        status: ActivityStatus::Unknown,
                        last_msg_to: None,
                        last_msg_from: None,
                        status_updated_at: None,
                        last_activity_at: None,
                    });
                }
            }
        }

        // Remove closed tabs (IDs no longer in TabUpdate)
        let closed: Vec<usize> = self
            .known_tabs
            .keys()
            .filter(|id| !seen_tab_ids.contains(id))
            .cloned()
            .collect();

        for tab_id in closed {
            if let Some(crew_tab) = self.known_tabs.remove(&tab_id) {
                if !crew_tab.user_defined {
                    eprintln!("[crew:{}:leader] Tab {} '{}' closed, name returns to pool", self.instance_id, tab_id, crew_tab.name);
                } else {
                    eprintln!("[crew:{}:leader] Tab {} '{}' closed (user-defined)", self.instance_id, tab_id, crew_tab.name);
                }
            }
        }

        // Broadcast updated state to renderers
        self.broadcast_state();
    }

    /// Resolve a terminal pane ID to the crew tab name that contains it.
    fn resolve_pane_name(&self, pane_id: u32) -> Option<String> {
        let manifest = self.pane_manifest.as_ref()?;
        let tab_pos = manifest.panes.iter().find_map(|(pos, panes)| {
            if panes.iter().any(|p| !p.is_plugin && p.id == pane_id) {
                Some(*pos)
            } else {
                None
            }
        })?;
        let tab = self.tabs.iter().find(|t| t.position == tab_pos)?;
        self.known_tabs.get(&tab.tab_id).map(|ct| ct.name.clone())
    }

    fn handle_external_status_update(&mut self, pipe_message: &PipeMessage) -> bool {
        // Try to parse as JSON first (for name-based routing)
        if let Some(payload) = &pipe_message.payload {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(to_name) = json.get("to").and_then(|v| v.as_str()) {
                    // Name-based message routing
                    eprintln!("[crew:{}:leader] Received message for '{}'", self.instance_id, to_name);
                    // TODO: Store messages for named tabs
                    return true;
                }
            }
        }

        // Parse key=value args format: "pane=ID,state=STATUS" or "name=NAME,state=STATUS"
        if let Some(state_str) = pipe_message.args.get("state") {
            // Try pane ID first
            if let Some(pane_id_str) = pipe_message.args.get("pane") {
                if let Ok(pane_id) = pane_id_str.parse::<u32>() {
                    return self.update_pane_status(pane_id, state_str);
                }
            }
            // Try name
            if let Some(name) = pipe_message.args.get("name") {
                return self.update_name_status(name, state_str);
            }
        }

        eprintln!("[crew:{}:leader] Unrecognized status pipe format", self.instance_id);
        false
    }

    fn update_name_status(&mut self, name: &str, state_str: &str) -> bool {
        // Parse activity status
        let new_status = match state_str {
            "unknown" => ActivityStatus::Unknown,
            "idle" => ActivityStatus::Idle,
            "working" => ActivityStatus::Working,
            "question" => ActivityStatus::Question,
            "sleeping" => ActivityStatus::Sleeping,
            "watching" => ActivityStatus::Watching,
            "attention" => ActivityStatus::Attention,
            _ => {
                eprintln!("[crew:{}:leader] Unrecognized status: {}", self.instance_id, state_str);
                return false;
            }
        };

        // Find tab by name
        let found = if let Some(crew_tab) = self.known_tabs.values_mut().find(|t| t.name == name) {
            let old = crew_tab.status.status_str();
            let changed = crew_tab.status != new_status;
            if changed {
                crew_tab.status = new_status;
                crew_tab.status_updated_at = Some(epoch_secs());
            }
            Some((old, changed))
        } else {
            None
        };
        if let Some((old_status, changed)) = found {
            self.log_event(serde_json::json!({
                "t": "status", "ts": epoch_secs(),
                "name": name, "old": old_status, "new": state_str,
                "changed": changed, "via": "name",
            }));
            if changed {
                eprintln!("[crew:{}:leader] Updating tab '{}' to status: {}", self.instance_id, name, state_str);
                self.broadcast_state();
                return true;
            }
        } else {
            self.log_event(serde_json::json!({
                "t": "status", "ts": epoch_secs(),
                "name": name, "new": state_str,
                "error": "tab not found", "via": "name",
            }));
            eprintln!("[crew:{}:leader] Tab '{}' not found", self.instance_id, name);
        }
        false
    }

    fn update_pane_status(&mut self, pane_id: u32, state_str: &str) -> bool {
        // Parse activity status
        let new_status = match state_str {
            "unknown" => ActivityStatus::Unknown,
            "idle" => ActivityStatus::Idle,
            "working" => ActivityStatus::Working,
            "question" => ActivityStatus::Question,
            "sleeping" => ActivityStatus::Sleeping,
            "watching" => ActivityStatus::Watching,
            "attention" => ActivityStatus::Attention,
            _ => {
                eprintln!("[crew:{}:leader] Unrecognized status: {}", self.instance_id, state_str);
                return false;
            }
        };

        // Find which tab contains this pane
        let tab_position = if let Some(manifest) = &self.pane_manifest {
            let result = manifest.panes.iter().find_map(|(tab_pos, panes)| {
                if panes.iter().any(|p| !p.is_plugin && p.id == pane_id) {
                    eprintln!("[crew:{}:leader] Found pane {} in tab position {}", self.instance_id, pane_id, tab_pos);
                    Some(*tab_pos)
                } else {
                    None
                }
            });

            if result.is_none() {
                eprintln!("[crew:{}:leader] Pane {} not found in manifest (tabs: {})",
                    self.instance_id, pane_id, manifest.panes.len());
            }

            result
        } else {
            eprintln!("[crew:{}:leader] No pane manifest available", self.instance_id);
            None
        };

        if let Some(tab_pos) = tab_position {
            eprintln!("[crew:{}:leader] Looking for tab at position {} in {} tabs",
                self.instance_id, tab_pos, self.tabs.len());

            // Map tab position to tab_id using TabInfo.tab_id
            let tab_id = self.tabs.iter()
                .find(|t| t.position == tab_pos)
                .map(|t| t.tab_id);

            if let Some(tab_id) = tab_id {
                // Update specific tab
                let (tab_name, old_status, changed) = if let Some(crew_tab) = self.known_tabs.get_mut(&tab_id) {
                    let old = crew_tab.status.status_str();
                    let changed = crew_tab.status != new_status;
                    let name = crew_tab.name.clone();
                    if changed {
                        crew_tab.status = new_status;
                        crew_tab.status_updated_at = Some(epoch_secs());
                    }
                    (name, old, changed)
                } else {
                    (String::new(), "", false)
                };
                if !tab_name.is_empty() {
                    self.log_event(serde_json::json!({
                        "t": "status", "ts": epoch_secs(),
                        "name": tab_name, "pane": pane_id,
                        "old": old_status, "new": state_str,
                        "changed": changed, "via": "pane",
                    }));
                    if changed {
                        eprintln!("[crew:{}:leader] Updating tab '{}' (id={}) to status: {}",
                            self.instance_id, tab_name, tab_id, state_str);
                        self.broadcast_state();
                        return true;
                    }
                }
            } else {
                self.log_event(serde_json::json!({
                    "t": "status", "ts": epoch_secs(),
                    "pane": pane_id, "new": state_str,
                    "error": "unmapped tab_position", "via": "pane",
                }));
                eprintln!("[crew:{}:leader] Could not map tab_position {} to tab_id", self.instance_id, tab_pos);
            }
        } else {
            self.log_event(serde_json::json!({
                "t": "status", "ts": epoch_secs(),
                "pane": pane_id, "new": state_str,
                "error": "pane not found", "via": "pane",
            }));
            eprintln!("[crew:{}:leader] Pane {} not found in any tab", self.instance_id, pane_id);
        }

        false
    }

    fn log_event(&self, entry: serde_json::Value) {
        // WASI /tmp maps to /tmp/zellij-{uid} on host; zellij-log/ is already there
        let log_path = "/tmp/zellij-log/zellij-crew-messages.log";
        let mut line = entry.to_string();
        line.push('\n');
        match OpenOptions::new().create(true).append(true).open(log_path) {
            Ok(mut f) => { let _ = f.write_all(line.as_bytes()); }
            Err(e) => eprintln!("[crew:{}:leader] Failed to write event log: {}", self.instance_id, e),
        }
    }

    fn handle_tell_message(&mut self, pipe_message: &PipeMessage) -> bool {
        let dest = match pipe_message.args.get("to") {
            Some(d) => d,
            None => {
                if let PipeSource::Cli(pipe_id) = &pipe_message.source {
                    cli_pipe_output(pipe_id, "error: missing 'to' argument\n");
                }
                return false;
            }
        };

        let message = match &pipe_message.payload {
            Some(p) if !p.is_empty() => p.as_str(),
            _ => {
                if let PipeSource::Cli(pipe_id) = &pipe_message.source {
                    cli_pipe_output(pipe_id, "error: missing message payload\n");
                }
                return false;
            }
        };

        // Resolve sender name from pane ID
        let sender = pipe_message.args.get("pane")
            .and_then(|id_str| id_str.parse::<u32>().ok())
            .and_then(|id| self.resolve_pane_name(id))
            .unwrap_or_else(|| {
                pipe_message.args.get("pane")
                    .map(|id| format!("pane {}", id))
                    .unwrap_or_else(|| "unknown".to_string())
            });

        // Find destination tab (case-insensitive), extract values to release borrow
        let (dest_name, dest_position) = match self.known_tabs.values()
            .find(|t| t.name.eq_ignore_ascii_case(dest))
        {
            Some(t) => (t.name.clone(), t.position),
            None => {
                if let PipeSource::Cli(pipe_id) = &pipe_message.source {
                    cli_pipe_output(pipe_id, &format!("error: tab '{}' not found\n", dest));
                }
                return false;
            }
        };

        // Assign message ID
        self.next_msg_id += 1;
        let msg_id = self.next_msg_id;

        // Find a terminal pane in the destination tab
        let dest_pane_id = match &self.pane_manifest {
            Some(manifest) => {
                manifest.panes.get(&dest_position)
                    .and_then(|panes| panes.iter().find(|p| !p.is_plugin))
                    .map(|p| p.id)
            }
            None => {
                if let PipeSource::Cli(pipe_id) = &pipe_message.source {
                    cli_pipe_output(pipe_id, "error: pane manifest not available\n");
                }
                return false;
            }
        };

        match dest_pane_id {
            Some(pane_id) => {
                let append = self.config.tell_append
                    .replace("{from}", &sender)
                    .replace("{to}", &dest_name)
                    .replace("{message}", message)
                    .replace("{id}", &msg_id.to_string());
                let formatted = format!(
                    "\n[CREW MESSAGE #{msg_id} from {sender}; to: {dest_name}] {message}\n{append}\n",
                );
                // Send message text now, delay Enter via timer so they
                // arrive as separate read() events on the receiving pty
                write_to_pane_id(formatted.into_bytes(), PaneId::Terminal(pane_id));
                self.pending_tell_enter = Some(pane_id);
                set_timeout(self.config.tell_delay_ms as f64 / 1000.0);
                self.log_event(serde_json::json!({
                    "t": "msg", "ts": epoch_secs(),
                    "id": msg_id, "from": sender, "to": dest_name,
                    "pane": pane_id, "msg": message,
                }));
                if let PipeSource::Cli(pipe_id) = &pipe_message.source {
                    cli_pipe_output(pipe_id, &format!(
                        "msg#{} sent to {} on pane {}\n", msg_id, dest_name, pane_id
                    ));
                }
                eprintln!("[crew:{}:leader] Delivered msg#{} from '{}' to '{}' (pane {})",
                    self.instance_id, msg_id, sender, dest_name, pane_id);

                // Track message timestamps for state query
                let now = epoch_secs();
                if let Some(dest_tab) = self.known_tabs.values_mut()
                    .find(|t| t.name == dest_name)
                {
                    dest_tab.last_msg_to = Some((msg_id, now));
                }
                if let Some(sender_tab) = self.known_tabs.values_mut()
                    .find(|t| t.name == sender)
                {
                    sender_tab.last_msg_from = Some((msg_id, now));
                }
            }
            None => {
                if let PipeSource::Cli(pipe_id) = &pipe_message.source {
                    cli_pipe_output(pipe_id, &format!("error: no terminal pane found in tab '{}'\n", dest));
                }
            }
        }

        false
    }
}

// ============================================================================
// Plugin Implementation
// ============================================================================

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // Generate a per-instance ID for log disambiguation across session resurrections
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        self.instance_id = format!("{:08x}", (nanos & 0xFFFFFFFF) as u32);

        self.config = Config::from_btreemap(&configuration);
        self.plugin_id = get_plugin_ids().plugin_id;

        eprintln!("[crew:{}:plugin{}] load() config={:?}", self.instance_id, self.plugin_id, configuration);

        // Don't call set_selectable(false) here - we need to remain selectable
        // so the user can focus this pane and grant permission on first run
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::MessageAndLaunchOtherPlugins,
            PermissionType::ReadCliPipes,
            PermissionType::WriteToStdin,
            PermissionType::ReadPaneContents,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
            EventType::Mouse,
            EventType::Timer,
            EventType::BeforeClose,
            EventType::PermissionRequestResult,
            EventType::PaneRenderReport,
        ]);

        // Election starts after permission grant (pipe_message_to_plugin
        // requires MessageAndLaunchOtherPlugins permission)
    }

    fn update(&mut self, event: Event) -> bool {
        eprintln!("[crew:{}] update() event={:?}", self.instance_id, std::mem::discriminant(&event));

        let mut should_render = false;
        match event {
            Event::ModeUpdate(mode_info) => {
                if self.mode_info != mode_info {
                    should_render = true;
                }
                self.mode_info = mode_info;
            }
            Event::TabUpdate(tabs) => {
                // Fallback: if leader died and we never got a new PermissionRequestResult,
                // trigger election on first TabUpdate we see as a leaderless renderer
                if !self.is_leader && !self.election_pending && self.tabs.is_empty() {
                    self.start_election();
                }
                // All instances: store tabs for rendering
                if let Some(active_tab_index) = tabs.iter().position(|t| t.active) {
                    let active_tab_idx = active_tab_index + 1;

                    if self.active_tab_idx != active_tab_idx || self.tabs != tabs {
                        should_render = true;
                    }
                    self.active_tab_idx = active_tab_idx;
                } else {
                    eprintln!("[crew:{}] Could not find active tab.", self.instance_id);
                }
                // Leader additionally manages tab names
                if self.is_leader {
                    self.handle_leader_tab_update(&tabs);
                }
                self.tabs = tabs;
            }
            Event::PaneUpdate(pane_manifest) => {
                if self.is_leader {
                    // Leader: store pane manifest for pane_id -> tab mapping
                    self.pane_manifest = Some(pane_manifest);
                }
                should_render = false;
            }
            Event::PaneRenderReport(pane_contents) => {
                if self.is_leader {
                    let now = epoch_secs();
                    for pane_id in pane_contents.keys() {
                        if let PaneId::Terminal(id) = pane_id {
                            // Map pane_id -> tab position via pane_manifest
                            let tab_pos = self.pane_manifest.as_ref().and_then(|m| {
                                m.panes.iter().find_map(|(pos, panes)| {
                                    if panes.iter().any(|p| !p.is_plugin && p.id == *id) {
                                        Some(*pos)
                                    } else {
                                        None
                                    }
                                })
                            });
                            // Map tab position -> tab_id via self.tabs
                            if let Some(pos) = tab_pos {
                                if let Some(tab_id) = self.tabs.iter()
                                    .find(|t| t.position == pos)
                                    .map(|t| t.tab_id)
                                {
                                    if let Some(crew_tab) = self.known_tabs.get_mut(&tab_id) {
                                        crew_tab.last_activity_at = Some(now);
                                        // Wake sleeping tabs on terminal output
                                        if crew_tab.status == ActivityStatus::Sleeping {
                                            eprintln!("[crew:{}:leader] Tab '{}' woke from sleeping (terminal activity)",
                                                self.instance_id, crew_tab.name);
                                            crew_tab.status = ActivityStatus::Idle;
                                            crew_tab.status_updated_at = Some(now);
                                            should_render = true;
                                            self.broadcast_state();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Event::Timer(_) => {
                if let Some(pane_id) = self.pending_tell_enter.take() {
                    write_to_pane_id(vec![b'\r'], PaneId::Terminal(pane_id));
                }
                if self.election_pending {
                    // No ack received within timeout, claim leadership
                    self.become_leader();
                    // Process current tabs now that we're leader
                    if !self.tabs.is_empty() {
                        let tabs = self.tabs.clone();
                        self.handle_leader_tab_update(&tabs);
                    }
                    should_render = true;
                }
                // Periodic activity check: transition stale tabs to sleeping
                if self.is_leader && self.config.idle_sleep_secs > 0 {
                    let now = epoch_secs();
                    let threshold = self.config.idle_sleep_secs;
                    let mut changed = false;
                    for crew_tab in self.known_tabs.values_mut() {
                        if crew_tab.status == ActivityStatus::Sleeping
                            || crew_tab.status == ActivityStatus::Unknown
                        {
                            continue;
                        }
                        let status_stale = crew_tab.status_updated_at
                            .map(|t| now.saturating_sub(t) >= threshold)
                            .unwrap_or(false);
                        let activity_stale = crew_tab.last_activity_at
                            .map(|t| now.saturating_sub(t) >= threshold)
                            .unwrap_or(true);
                        if status_stale && activity_stale {
                            eprintln!("[crew:{}:leader] Tab '{}' idle too long, transitioning to sleeping",
                                self.instance_id, crew_tab.name);
                            crew_tab.status = ActivityStatus::Sleeping;
                            crew_tab.status_updated_at = Some(now);
                            changed = true;
                        }
                    }
                    if changed {
                        self.broadcast_state();
                        should_render = true;
                    }
                    // Re-arm periodic timer
                    set_timeout(30.0);
                }
            }
            Event::BeforeClose => {
                self.resign_leadership();
            }
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                eprintln!("[crew:{}] PermissionRequestResult::Granted received", self.instance_id);
                set_selectable(false);
                subscribe(&[
                    EventType::TabUpdate,
                    EventType::ModeUpdate,
                    EventType::Mouse,
                    EventType::Timer,
                    EventType::BeforeClose,
                    EventType::PaneRenderReport,
                ]);
                // Start election now that pipe messaging is available
                self.start_election();
                should_render = true;
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                eprintln!("[crew:{}] Permission denied - plugin will not function properly", self.instance_id);
            }
            Event::Mouse(me) => {
                match me {
                    Mouse::LeftClick(_, col) => {
                        let tab_to_focus =
                            get_tab_to_focus(&self.tab_line, self.active_tab_idx, col);
                        if let Some(idx) = tab_to_focus {
                            switch_tab_to(idx.try_into().unwrap());
                        }
                    }
                    Mouse::ScrollUp(_) => {
                        switch_tab_to(min(self.active_tab_idx + 1, self.tabs.len()) as u32);
                    }
                    Mouse::ScrollDown(_) => {
                        switch_tab_to(max(self.active_tab_idx.saturating_sub(1), 1) as u32);
                    }
                    _ => {}
                }
            }
            _ => {
                eprintln!("[crew:{}] Got unrecognized event: {:?}", self.instance_id, event);
            }
        }
        if self.tabs.is_empty() {
            should_render = false;
        }
        should_render
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        eprintln!("[crew:{}] pipe() received: is_leader={} name='{}' source={:?}",
            self.instance_id, self.is_leader, pipe_message.name, pipe_message.source);

        // ---- Election protocol messages ----

        if pipe_message.name == MSG_LEADER_PING {
            if let Some(payload) = &pipe_message.payload {
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(payload) {
                    let sender_id = msg["plugin_id"].as_u64().unwrap_or(0) as u32;
                    if sender_id != self.plugin_id && self.is_leader {
                        // We're the leader, respond with ack + state
                        eprintln!("[crew:{}:leader] Acking ping from plugin {}", self.instance_id, sender_id);
                        let state: Vec<&CrewTabState> = self.known_tabs.values().collect();
                        let ack = serde_json::json!({
                            "plugin_id": self.plugin_id,
                            "state": state,
                        });
                        pipe_message_to_plugin(
                            MessageToPlugin::new(MSG_LEADER_ACK)
                                .with_payload(serde_json::to_string(&ack).unwrap_or_default()),
                        );
                    }
                }
            }
            return false;
        }

        if pipe_message.name == MSG_LEADER_ACK {
            if let Some(payload) = &pipe_message.payload {
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(payload) {
                    let sender_id = msg["plugin_id"].as_u64().unwrap_or(0) as u32;
                    if sender_id != self.plugin_id && self.election_pending {
                        // Cancel election, stay renderer
                        eprintln!("[crew:{}] Received ack from leader (plugin {}), staying renderer",
                            self.instance_id, sender_id);
                        self.election_pending = false;
                        // Parse state from ack for immediate rendering
                        if let Some(state_val) = msg.get("state") {
                            if let Ok(tabs) = serde_json::from_value::<Vec<CrewTabState>>(state_val.clone()) {
                                self.received_tabs = tabs;
                                return true; // Re-render with received state
                            }
                        }
                    }
                }
            }
            return false;
        }

        if pipe_message.name == MSG_LEADER_CLAIM {
            if let Some(payload) = &pipe_message.payload {
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(payload) {
                    let claimer_id = msg["plugin_id"].as_u64().unwrap_or(0) as u32;
                    if claimer_id != self.plugin_id {
                        if self.is_leader && claimer_id > self.plugin_id {
                            // Higher ID wins, we yield
                            eprintln!("[crew:{}:leader] Yielding to plugin {} (higher ID)",
                                self.instance_id, claimer_id);
                            self.is_leader = false;
                        }
                        if self.election_pending && claimer_id > self.plugin_id {
                            // Higher ID is claiming, cancel our election
                            eprintln!("[crew:{}] Higher-ID plugin {} claimed, canceling election",
                                self.instance_id, claimer_id);
                            self.election_pending = false;
                        }
                    }
                }
            }
            return false;
        }

        if pipe_message.name == MSG_LEADER_RESIGN {
            if let Some(payload) = &pipe_message.payload {
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(payload) {
                    let resigner_id = msg["plugin_id"].as_u64().unwrap_or(0) as u32;
                    if resigner_id != self.plugin_id {
                        eprintln!("[crew:{}] Leader (plugin {}) resigned, starting new election",
                            self.instance_id, resigner_id);
                        // Parse inherited state
                        if let Some(state_val) = msg.get("state") {
                            if let Ok(tabs) = serde_json::from_value::<Vec<CrewTabState>>(state_val.clone()) {
                                let map: HashMap<usize, CrewTabState> = tabs.into_iter()
                                    .map(|t| (t.tab_id, t))
                                    .collect();
                                self.inherited_state = Some(map);
                            }
                        }
                        // Start new election
                        self.start_election();
                    }
                }
            }
            return false;
        }

        // ---- Internal crew-state broadcasts ----

        if !self.is_leader && pipe_message.name == "crew-state" {
            // Receiving state from the leader proves a leader exists
            if self.election_pending {
                eprintln!("[crew:{}] Received crew-state during election, canceling (leader exists)",
                    self.instance_id);
                self.election_pending = false;
            }
            if let Some(payload) = pipe_message.payload {
                match serde_json::from_str::<Vec<CrewTabState>>(&payload) {
                    Ok(tabs) => {
                        eprintln!("[crew:{}:renderer] Received state via pipe: {} tabs", self.instance_id, tabs.len());
                        self.received_tabs = tabs;
                        return true; // Request render
                    }
                    Err(e) => {
                        eprintln!("[crew:{}:renderer] ERROR: Failed to parse state: {}", self.instance_id, e);
                    }
                }
            }
            return false;
        }

        // ---- External zellij-crew:status messages (leader only) ----

        if self.is_leader && pipe_message.name == "zellij-crew:status" {
            // Help command - check args or payload
            let is_help = pipe_message.args.contains_key("help")
                || pipe_message.payload.as_deref() == Some("help");

            if is_help {
                if let PipeSource::Cli(pipe_id) = &pipe_message.source {
                    let help = r#"zellij-crew:status - Update tab activity status

Usage:
  zellij pipe --name zellij-crew:status --args "pane=PANE_ID,state=STATE"
  zellij pipe --name zellij-crew:status --args "name=NAME,state=STATE"

States:
  unknown   🫥  No status / agent exited
  idle      🥱  Agent idle
  working   🤖  Agent working
  question  🙋  Agent has a question
  sleeping  😴  Agent sleeping/paused
  watching  👀  Agent watching/monitoring
  attention 🔔  Needs attention

Config (in plugin KDL):
  status_unknown ""        Hide indicator when unknown
  status_working "WRK"     Custom text shown as [WRK]
  (set any status_* to "" to suppress the [brackets] entirely)

Commands:
  --args help               Show this help
  --args list               List all tabs (alias: ls)
  --args format=json,list   Output in JSON format
  --args format=json,state  Detailed per-tab state (pane info, msg tracking)

Examples:
  zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=working"
  zellij pipe --name zellij-crew:status --args "name=Alice,state=attention"
"#;
                    cli_pipe_output(pipe_id, help);
                }
                return false;
            }

            // List command - show all tabs
            let is_list = pipe_message.args.contains_key("list")
                || pipe_message.args.contains_key("ls")
                || pipe_message.payload.as_deref() == Some("list")
                || pipe_message.payload.as_deref() == Some("ls");

            if is_list {
                if let PipeSource::Cli(pipe_id) = &pipe_message.source {
                    let want_json = pipe_message.args.get("format").map(|s| s.as_str()) == Some("json");

                    let mut tabs: Vec<_> = self.known_tabs.values().collect();
                    tabs.sort_by_key(|t| t.tab_id);

                    let output = if want_json {
                        // JSON format
                        let json_tabs: Vec<_> = tabs.iter().map(|tab| {
                            let status_str = match tab.status {
                                ActivityStatus::Unknown => "unknown",
                                ActivityStatus::Idle => "idle",
                                ActivityStatus::Working => "working",
                                ActivityStatus::Question => "question",
                                ActivityStatus::Sleeping => "sleeping",
                                ActivityStatus::Watching => "watching",
                                ActivityStatus::Attention => "attention",
                            };
                            serde_json::json!({
                                "id": tab.tab_id,
                                "pos": tab.position,
                                "name": tab.name,
                                "status": status_str
                            })
                        }).collect();
                        format!("{}\n", serde_json::to_string_pretty(&json_tabs).unwrap_or_else(|_| "[]".to_string()))
                    } else {
                        // Human-readable format
                        let mut out = String::from("ID\tName\tStatus\n");
                        out.push_str("--\t----\t------\n");

                        for tab in tabs {
                            let status_str = match tab.status {
                                ActivityStatus::Unknown => "🫥 unknown",
                                ActivityStatus::Idle => "🥱 idle",
                                ActivityStatus::Working => "🤖 working",
                                ActivityStatus::Question => "🙋 question",
                                ActivityStatus::Sleeping => "😴 sleeping",
                                ActivityStatus::Watching => "👀 watching",
                                ActivityStatus::Attention => "🔔 attention",
                            };
                            out.push_str(&format!("{}\t{}\t{}\n", tab.tab_id, tab.name, status_str));
                        }

                        if self.known_tabs.is_empty() {
                            out.push_str("(no tabs)\n");
                        }
                        out
                    };

                    cli_pipe_output(pipe_id, &output);
                }
                return false;
            }

            // State command - detailed per-tab state for agent coordination
            // Triggered by: --args "format=json,state" (with no pane/name keys)
            // or explicitly: --args "state_query" (used by CLI)
            let is_state = pipe_message.args.contains_key("state_query")
                || (pipe_message.args.get("format").map(|s| s.as_str()) == Some("json")
                    && pipe_message.args.contains_key("state")
                    && !pipe_message.args.contains_key("pane")
                    && !pipe_message.args.contains_key("name"));

            if is_state {
                if let PipeSource::Cli(pipe_id) = &pipe_message.source {
                    let mut tabs: Vec<_> = self.known_tabs.values().collect();
                    tabs.sort_by_key(|t| t.position);

                    let json_tabs: Vec<_> = tabs.iter().map(|tab| {
                        let status_str = tab.status.status_str();

                        let msg_to = tab.last_msg_to.map(|(id, ts)| {
                            serde_json::json!({"id": id, "ts": ts})
                        });
                        let msg_from = tab.last_msg_from.map(|(id, ts)| {
                            serde_json::json!({"id": id, "ts": ts})
                        });

                        // Find terminal pane info for this tab
                        let pane_info = self.pane_manifest.as_ref().and_then(|manifest| {
                            manifest.panes.get(&tab.position).and_then(|panes| {
                                panes.iter().find(|p| !p.is_plugin).map(|p| {
                                    serde_json::json!({
                                        "id": p.id,
                                        "title": p.title,
                                        "is_focused": p.is_focused,
                                        "exited": p.exited,
                                        "exit_status": p.exit_status,
                                        "rows": p.pane_content_rows,
                                        "cols": p.pane_content_columns,
                                    })
                                })
                            })
                        });

                        serde_json::json!({
                            "id": tab.tab_id,
                            "pos": tab.position,
                            "name": tab.name,
                            "status": status_str,
                            "status_updated_at": tab.status_updated_at,
                            "last_activity_at": tab.last_activity_at,
                            "last_msg_to": msg_to,
                            "last_msg_from": msg_from,
                            "pane": pane_info,
                        })
                    }).collect();

                    let output = format!("{}\n",
                        serde_json::to_string_pretty(&json_tabs)
                            .unwrap_or_else(|_| "[]".to_string()));
                    cli_pipe_output(pipe_id, &output);
                }
                return false;
            }

            return self.handle_external_status_update(&pipe_message);
        }

        // ---- External zellij-crew:msg messages (leader only) ----

        if self.is_leader && pipe_message.name == "zellij-crew:msg" {
            return self.handle_tell_message(&pipe_message);
        }

        false
    }

    fn render(&mut self, _rows: usize, cols: usize) {
        if self.tabs.is_empty() {
            // Don't render anything - let zellij show its permission dialog cleanly
            return;
        }

        // Build display names: leader uses known_tabs, renderer uses received_tabs
        let names: Vec<String> = self.tabs
            .iter()
            .map(|tab| {
                let crew_state: Option<&CrewTabState> = if self.is_leader {
                    // Leader: direct lookup by stable tab_id
                    self.known_tabs.get(&tab.tab_id)
                } else {
                    // Renderer: find in broadcast state by tab_id
                    self.received_tabs.iter().find(|ct| ct.tab_id == tab.tab_id)
                };

                if let Some(crew_tab) = crew_state {
                    match self.config.indicator_for(&crew_tab.status) {
                        Some(ind) => format!("{} [{}]", crew_tab.name, ind),
                        None => crew_tab.name.clone(),
                    }
                } else {
                    // No crew state yet, use Unknown's indicator config
                    match self.config.indicator_for(&ActivityStatus::Unknown) {
                        Some(ind) => format!("{} [{}]", tab.name, ind),
                        None => tab.name.to_string(),
                    }
                }
            })
            .collect();

        let mut all_tabs: Vec<LinePart> = vec![];
        let mut active_tab_index = 0;
        let mut is_alternate_tab = false;

        for (i, t) in self.tabs.iter().enumerate() {
            let tabname = names[i].clone();

            if t.active && self.mode_info.mode == InputMode::RenameTab {
                active_tab_index = t.position;
            } else if t.active {
                active_tab_index = t.position;
            }

            let tab = tab_style(
                tabname,
                t,
                is_alternate_tab,
                self.mode_info.style.colors,
                self.mode_info.capabilities,
            );
            is_alternate_tab = !is_alternate_tab;
            all_tabs.push(tab);
        }

        let background = self.mode_info.style.colors.text_unselected.background;

        self.tab_line = tab_line(
            self.mode_info.session_name.as_deref(),
            all_tabs,
            active_tab_index,
            cols.saturating_sub(1),
            self.mode_info.style.colors,
            self.mode_info.capabilities,
            self.mode_info.style.hide_session_name,
            self.tabs.iter().find(|t| t.active),
            &self.mode_info,
            self.config.hide_swap_layout_indication,
            &background,
        );

        let output = self
            .tab_line
            .iter()
            .fold(String::new(), |output, part| output + &part.part);

        match background {
            PaletteColor::Rgb((r, g, b)) => {
                print!("{}\u{1b}[48;2;{};{};{}m\u{1b}[0K", output, r, g, b);
            }
            PaletteColor::EightBit(color) => {
                print!("{}\u{1b}[48;5;{}m\u{1b}[0K", output, color);
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

// Tests disabled during leader election architecture migration
// TODO: Rewrite tests for new architecture
//
// Test areas needed:
// - Name allocation (round-robin, fill-in)
// - Leader election protocol
// - State broadcast and inheritance
// - Pipe protocol handling (status updates, list command)
// - Tab rename confirmation loop
