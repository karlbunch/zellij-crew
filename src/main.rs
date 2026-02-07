// zellij-crew: Tab-bar plugin with named tabs and activity indicators
// Based on zellij-tab-bar-indexed (MIT license)

mod line;
mod tab;

use std::cmp::{max, min};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryInto;

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

        Config {
            names,
            mode,
            hide_swap_layout_indication,
            status_indicators,
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
}

impl Default for ActivityStatus {
    fn default() -> Self {
        ActivityStatus::Unknown
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrewTabState {
    tab_id: u32,                     // Stable ID from "Tab #N" (redundant with HashMap key but explicit)
    position: usize,                 // Current position (updates when tabs reorder)
    name: String,                    // Current name ("Alice" after rename, "Tab #5" before)

    // ADR: Why pending_rename flag?
    // Prevents infinite rename loops: when we call rename_tab(), the next TabUpdate
    // may still show the old name (rename not processed yet). We track pending renames
    // to avoid re-renaming the same tab. Once TabUpdate shows the new name, we confirm
    // by matching against pending_rename and clearing the flag.
    #[serde(skip)]                   // Don't serialize internal rename tracking
    pending_rename: Option<String>,  // Some("Alice") when rename sent, waiting for confirmation

    user_defined: bool,              // true if user named it, false if from our pool
    status: ActivityStatus,          // Current activity status
}

#[derive(Default)]
struct State {
    // Common state (both modes)
    mode_info: ModeInfo,
    config: Config,
    first_render_done: bool,
    is_leader: bool,

    // Leader-only state
    // ADR: Why HashMap<u32, CrewTabState> with tab_id as key?
    // - tab_id is stable (never changes, even when tabs close/reorder)
    // - Parsed from "Tab #N" default name (N is the tab_id)
    // - Required for rename_tab(tab_id, name) API
    // - O(1) lookup when confirming renames or updating status
    known_tabs: HashMap<u32, CrewTabState>,  // tab_id -> CrewTabState
    pane_manifest: Option<PaneManifest>,     // For mapping pane_id -> tab
    leader_tabs: Vec<TabInfo>,               // Current tabs (for pane_id -> tab_id mapping)
    last_assigned_idx: Option<usize>,

    // Renderer-only state
    received_tabs: Vec<CrewTabState>,  // From leader broadcast
    tabs: Vec<TabInfo>,            // From TabUpdate (for rendering)
    active_tab_idx: usize,
    tab_line: Vec<LinePart>,       // Cached for mouse clicks
}

// ============================================================================
// Helper Functions
// ============================================================================

fn parse_default_name(name: &str) -> Option<u32> {
    // "Tab #5" → Some(5) (tab_id for rename_tab)
    if name.starts_with("Tab #") {
        name[5..].parse().ok()
    } else {
        None
    }
}

// ============================================================================
// Leader State Management
// ============================================================================

impl State {
    // ADR: Why broadcast instead of query?
    // We could have renderers query the leader for state, but broadcasting is more efficient:
    // - Query: N renderers × M queries/sec = N×M messages (grows with renderer count)
    // - Broadcast: 1 broadcast on state change (typically 0-5/sec, independent of renderer count)
    // - Mirrors how zellij handles multi-user focus indicators (broadcast TabInfo.other_focused_clients)
    // - Simpler: renderers are stateless, just paint what they receive
    // - No race conditions: all renderers see same state at same time
    // Trade-off: Small delay when new renderer starts (waits for next state change)
    fn broadcast_state(&self) {
        if !self.is_leader {
            return; // Only leader broadcasts
        }

        let tabs: Vec<&CrewTabState> = self.known_tabs.values().collect();

        if let Ok(json) = serde_json::to_string(&tabs) {
            eprintln!("[crew:leader] Broadcasting state: {} tabs", tabs.len());

            // Send to all crew instances via plugin_url
            pipe_message_to_plugin(
                MessageToPlugin::new("crew-state")
                    .with_plugin_url("crew")  // Routes to all instances with this URL
                    .with_payload(json)
            );
        } else {
            eprintln!("[crew:leader] ERROR: Failed to serialize state");
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
        eprintln!("[crew:leader] Processing {} tabs", tabs.len());

        // Store tabs for pane_id -> tab_id mapping
        self.leader_tabs = tabs.to_vec();

        // Track which tab IDs we've seen in this update
        let mut seen_tab_ids = HashSet::new();

        // PASS 1: Match existing tabs by name (handles renames) and position (handles moves)
        for tab in tabs {
            let mut matched = false;

            // Try to match by name first
            for (tab_id, crew_tab) in self.known_tabs.iter_mut() {
                if let Some(pending) = &crew_tab.pending_rename {
                    if pending == &tab.name {
                        // Pending rename confirmed
                        eprintln!("[crew:leader] Rename confirmed: Tab #{} -> {} (pos {})", tab_id, tab.name, tab.position);
                        crew_tab.name = tab.name.clone();
                        crew_tab.position = tab.position;
                        crew_tab.pending_rename = None;
                        seen_tab_ids.insert(*tab_id);
                        matched = true;
                        break;
                    }
                } else if crew_tab.name == tab.name {
                    // Name match - update position if changed
                    if crew_tab.position != tab.position {
                        eprintln!("[crew:leader] Tab #{} '{}' moved from position {} to {}",
                            tab_id, tab.name, crew_tab.position, tab.position);
                        crew_tab.position = tab.position;
                    }
                    seen_tab_ids.insert(*tab_id);
                    matched = true;
                    break;
                }
            }

            if matched {
                continue;
            }

            // Try to match by position (catches user renames of existing tabs)
            for (tab_id, crew_tab) in self.known_tabs.iter_mut() {
                if crew_tab.position == tab.position && crew_tab.pending_rename.is_none() {
                    // Same position, different name - user renamed it
                    eprintln!("[crew:leader] Tab #{} at position {} renamed: '{}' -> '{}' (user-defined)",
                        tab_id, tab.position, crew_tab.name, tab.name);
                    crew_tab.name = tab.name.clone();
                    crew_tab.user_defined = true;
                    seen_tab_ids.insert(*tab_id);
                    matched = true;
                    break;
                }
            }

            if matched {
                continue;
            }

            // No match by name or position - this is a new tab
            // Try to extract tab_id from default name, or infer it
            let tab_id = if let Some(id) = parse_default_name(&tab.name) {
                id
            } else {
                // User-defined name on new tab - infer tab_id from missing IDs
                // Find the lowest tab_id not in known_tabs
                let used_ids: HashSet<u32> = self.known_tabs.keys().cloned().collect();
                (1..=100).find(|id| !used_ids.contains(id)).unwrap_or(tab.position as u32)
            };

            if parse_default_name(&tab.name).is_some() && !self.known_tabs.contains_key(&tab_id) {
                // New tab with default name - allocate a name from pool
                if let Some(new_name) = self.allocate_from_pool() {
                    eprintln!("[crew:leader] New tab: renaming Tab #{} -> {} (pos {})", tab_id, new_name, tab.position);
                    rename_tab(tab_id, new_name.clone());

                    self.known_tabs.insert(
                        tab_id,
                        CrewTabState {
                            tab_id,
                            position: tab.position,
                            name: tab.name.clone(),
                            pending_rename: Some(new_name),
                            user_defined: false,
                            status: ActivityStatus::Unknown,
                        },
                    );
                    seen_tab_ids.insert(tab_id);
                } else {
                    eprintln!("[crew:leader] Pool exhausted, leaving Tab #{} unnamed", tab_id);
                }
            } else if !seen_tab_ids.contains(&tab_id) {
                // New tab with user-defined name - track it
                eprintln!("[crew:leader] New tab with user-defined name: Tab #{} '{}' (pos {})",
                    tab_id, tab.name, tab.position);
                self.known_tabs.insert(
                    tab_id,
                    CrewTabState {
                        tab_id,
                        position: tab.position,
                        name: tab.name.clone(),
                        pending_rename: None,
                        user_defined: true,
                        status: ActivityStatus::Unknown,
                    },
                );
                seen_tab_ids.insert(tab_id);
            }
        }

        // PASS 2: Remove closed tabs
        let closed: Vec<u32> = self
            .known_tabs
            .keys()
            .filter(|tab_id| !seen_tab_ids.contains(tab_id))
            .cloned()
            .collect();

        for tab_id in closed {
            if let Some(crew_tab) = self.known_tabs.remove(&tab_id) {
                if !crew_tab.user_defined {
                    eprintln!("[crew:leader] Tab #{} '{}' closed, name returns to pool", tab_id, crew_tab.name);
                } else {
                    eprintln!("[crew:leader] Tab #{} '{}' closed (user-defined)", tab_id, crew_tab.name);
                }
            }
        }

        // Broadcast updated state to renderers
        self.broadcast_state();
    }

    fn handle_external_status_update(&mut self, pipe_message: &PipeMessage) -> bool {
        // Try to parse as JSON first (for name-based routing)
        if let Some(payload) = &pipe_message.payload {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(to_name) = json.get("to").and_then(|v| v.as_str()) {
                    // Name-based message routing
                    eprintln!("[crew:leader] Received message for '{}'", to_name);
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

        eprintln!("[crew:leader] Unrecognized status pipe format");
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
                eprintln!("[crew:leader] Unrecognized status: {}", state_str);
                return false;
            }
        };

        // Find tab by name
        if let Some(crew_tab) = self.known_tabs.values_mut().find(|t| t.name == name) {
            if crew_tab.status != new_status {
                eprintln!("[crew:leader] Updating tab '{}' to status: {:?}", name, new_status);
                crew_tab.status = new_status;
                self.broadcast_state();
                return true;
            }
        } else {
            eprintln!("[crew:leader] Tab '{}' not found", name);
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
                eprintln!("[crew:leader] Unrecognized status: {}", state_str);
                return false;
            }
        };

        // Find which tab contains this pane
        let tab_position = if let Some(manifest) = &self.pane_manifest {
            let result = manifest.panes.iter().find_map(|(tab_pos, panes)| {
                if panes.iter().any(|p| !p.is_plugin && p.id == pane_id) {
                    eprintln!("[crew:leader] Found pane {} in tab position {}", pane_id, tab_pos);
                    Some(*tab_pos)
                } else {
                    None
                }
            });

            if result.is_none() {
                eprintln!("[crew:leader] Pane {} not found in manifest (tabs: {})",
                    pane_id, manifest.panes.len());
            }

            result
        } else {
            eprintln!("[crew:leader] No pane manifest available");
            None
        };

        if let Some(tab_pos) = tab_position {
            eprintln!("[crew:leader] Looking for tab at position {} in {} tabs",
                tab_pos, self.leader_tabs.len());

            // Map tab position to tab_id from current tabs
            let tab_at_pos = self.leader_tabs.iter().find(|t| t.position == tab_pos);

            if let Some(tab) = tab_at_pos {
                eprintln!("[crew:leader] Tab at position {}: name='{}' active={}",
                    tab_pos, tab.name, tab.active);
            }

            if let Some(tab_id) = tab_at_pos.and_then(|t| parse_default_name(&t.name).or_else(|| {
                    // Tab already renamed, find it in known_tabs by name
                    self.known_tabs.iter()
                        .find(|(_, ct)| ct.name == t.name)
                        .map(|(id, _)| *id)
                }))
            {
                // Update specific tab
                if let Some(crew_tab) = self.known_tabs.get_mut(&tab_id) {
                    if crew_tab.status != new_status {
                        eprintln!("[crew:leader] Updating tab '{}' (id={}) to status: {:?}",
                            crew_tab.name, tab_id, new_status);
                        crew_tab.status = new_status;
                        self.broadcast_state();
                        return true;
                    }
                }
            } else {
                eprintln!("[crew:leader] Could not map tab_position {} to tab_id", tab_pos);
            }
        } else {
            eprintln!("[crew:leader] Pane {} not found in any tab", pane_id);
        }

        false
    }
}

// Name allocation logic is in allocate_from_pool() (lines 191-227)
// Old per-tab iteration approach was removed during leader/renderer refactor

// ============================================================================
// Plugin Implementation
// ============================================================================

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.config = Config::from_btreemap(&configuration);

        let my_id = get_plugin_ids().plugin_id;
        eprintln!("[crew:{}] load() config={:?}", my_id, configuration);

        // Don't call set_selectable(false) here - we need to remain selectable
        // so the user can focus this pane and grant permission on first run
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::MessageAndLaunchOtherPlugins,
            PermissionType::ReadCliPipes,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
            EventType::Mouse,
            EventType::PermissionRequestResult,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        eprintln!("[crew] update() event={:?}", std::mem::discriminant(&event));

        let mut should_render = false;
        match event {
            Event::ModeUpdate(mode_info) => {
                if self.mode_info != mode_info {
                    should_render = true;
                }
                self.mode_info = mode_info;
            }
            Event::TabUpdate(tabs) => {
                if self.is_leader {
                    // Leader: manage tab names
                    self.handle_leader_tab_update(&tabs);
                    // Leader doesn't render
                    should_render = false;
                } else {
                    // Renderer: store tabs for rendering
                    if let Some(active_tab_index) = tabs.iter().position(|t| t.active) {
                        let active_tab_idx = active_tab_index + 1;

                        if self.active_tab_idx != active_tab_idx || self.tabs != tabs {
                            should_render = true;
                        }
                        self.active_tab_idx = active_tab_idx;
                        self.tabs = tabs;
                    } else {
                        eprintln!("[crew:renderer] Could not find active tab.");
                    }
                }
            }
            Event::PaneUpdate(pane_manifest) => {
                if self.is_leader {
                    // Leader: store pane manifest for pane_id -> tab mapping
                    self.pane_manifest = Some(pane_manifest);
                }
                // Renderers don't need pane info
                should_render = false;
            }
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                if !self.is_leader {
                    set_selectable(false);
                }
                subscribe(&[
                    EventType::TabUpdate,
                    EventType::ModeUpdate,
                    EventType::Mouse,
                ]);
                should_render = !self.is_leader;
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                eprintln!("[crew] Permission denied - plugin will not function properly");
            }
            Event::Mouse(me) => {
                if self.is_leader {
                    return false;  // Leader doesn't handle mouse
                }
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
                eprintln!("[crew] Got unrecognized event: {:?}", event);
            }
        }
        if self.tabs.is_empty() {
            should_render = false;
        }
        should_render
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        eprintln!("[crew] pipe() received: is_leader={} name='{}' source={:?}",
            self.is_leader, pipe_message.name, pipe_message.source);

        // Renderers: receive internal crew-state broadcasts
        if !self.is_leader && pipe_message.name == "crew-state" {
            if let Some(payload) = pipe_message.payload {
                match serde_json::from_str::<Vec<CrewTabState>>(&payload) {
                    Ok(tabs) => {
                        eprintln!("[crew:renderer] Received state via pipe: {} tabs", tabs.len());
                        self.received_tabs = tabs;
                        return true; // Request render
                    }
                    Err(e) => {
                        eprintln!("[crew:renderer] ERROR: Failed to parse state: {}", e);
                    }
                }
            }
            return false;
        }

        // Leader: handle external zellij-crew:status messages
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
  --args help              Show this help
  --args list              List all tabs (alias: ls)
  --args format=json,list  Output in JSON format

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

            return self.handle_external_status_update(&pipe_message);
        }

        false
    }

    fn render(&mut self, rows: usize, cols: usize) {
        // ADR: Why rows > 1 for leader detection?
        // Through testing, we discovered that load_plugins instances receive a larger virtual
        // pane on first render (rows=20 even after permissions cached), while layout-based
        // tab-bar instances get rows ≤ 1. This allows a single WASM binary to serve both
        // roles without config changes. Alternative approaches considered:
        // - Separate binaries: Code duplication, harder to maintain
        // - Config flag: User must configure twice (error-prone)
        // - Plugin ID detection: IDs are not deterministic across restarts
        // The rows heuristic is robust and requires no user configuration.
        if !self.first_render_done {
            self.first_render_done = true;
            self.is_leader = rows > 1;
            eprintln!("[crew] FIRST render() rows={} cols={} is_leader={}", rows, cols, self.is_leader);
        }

        if self.is_leader {
            // Leader doesn't render anything
            return;
        }

        if self.tabs.is_empty() {
            // Don't render anything - let zellij show its permission dialog cleanly
            return;
        }

        // Renderer: get names from received_tabs (leader state) or fall back to TabUpdate
        let names: Vec<String> = self.tabs
            .iter()
            .map(|tab| {
                // Try to find crew state for this tab
                let crew_state = if let Some(tab_id) = parse_default_name(&tab.name) {
                    // Tab still has default name, look up by tab_id
                    self.received_tabs.iter().find(|ct| ct.tab_id == tab_id)
                } else {
                    // Tab has been renamed, look up by name
                    self.received_tabs.iter().find(|ct| ct.name == tab.name)
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

// Tests disabled during leader/renderer architecture migration
// TODO: Rewrite tests for new architecture (see TESTING.md)
//
// Test areas needed:
// - Name allocation (round-robin, fill-in)
// - Leader/renderer state broadcast
// - Pipe protocol handling (status updates, list command)
// - Tab rename confirmation loop
