// zellij-crew: Tab-bar plugin with named tabs and activity indicators
// Based on zellij-tab-bar-indexed (MIT license)

mod line;
mod tab;

use std::cmp::{max, min};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::TryInto;

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
    show_position: bool,
    hide_swap_layout_indication: bool,
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

        let show_position = config
            .get("show_position")
            .map(|s| s == "true")
            .unwrap_or(false);

        let hide_swap_layout_indication = config
            .get("hide_swap_layout_indication")
            .map(|s| s == "true")
            .unwrap_or(false);

        Config {
            names,
            mode,
            show_position,
            hide_swap_layout_indication,
        }
    }
}

// ============================================================================
// Plugin State
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum ActivityStatus {
    Unknown,
    Idle,
    Working,
    Question,
}

impl Default for ActivityStatus {
    fn default() -> Self {
        ActivityStatus::Unknown
    }
}

#[derive(Debug, Clone)]
struct CrewTabState {
    tab_id: u32,                     // Stable ID from "Tab #N" (redundant with HashMap key but explicit)
    name: String,                    // Current name ("Alice" after rename, "Tab #5" before)
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
    known_tabs: HashMap<u32, CrewTabState>,  // tab_id -> CrewTabState
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

        // Track which tab IDs still exist
        let mut current_tab_ids = HashSet::new();

        // Process each tab
        for tab in tabs {
            if let Some(tab_id) = parse_default_name(&tab.name) {
                // Default name "Tab #N"
                current_tab_ids.insert(tab_id);

                if let Some(crew_tab) = self.known_tabs.get_mut(&tab_id) {
                    // Known tab, check if still waiting for rename
                    if crew_tab.pending_rename.is_some() {
                        eprintln!("[crew:leader] Tab #{} still pending rename", tab_id);
                    }
                } else {
                    // New tab, needs renaming
                    if let Some(new_name) = self.allocate_from_pool() {
                        eprintln!("[crew:leader] Renaming Tab #{} -> {}", tab_id, new_name);
                        rename_tab(tab_id, new_name.clone());

                        self.known_tabs.insert(
                            tab_id,
                            CrewTabState {
                                tab_id,
                                name: tab.name.clone(),  // Still "Tab #N" until confirmed
                                pending_rename: Some(new_name),
                                user_defined: false,
                                status: ActivityStatus::Unknown,
                            },
                        );
                    } else {
                        eprintln!("[crew:leader] Pool exhausted, leaving Tab #{} unnamed", tab_id);
                    }
                }
            } else {
                // Non-default name (already renamed or user-defined)
                // Try to find which tab this is by checking pending_rename
                let mut found_tab_id = None;
                for (id, crew_tab) in self.known_tabs.iter_mut() {
                    if let Some(pending) = &crew_tab.pending_rename {
                        if pending == &tab.name {
                            // Rename confirmed!
                            eprintln!("[crew:leader] Rename confirmed: Tab #{} -> {}", id, tab.name);
                            crew_tab.name = tab.name.clone();
                            crew_tab.pending_rename = None;
                            found_tab_id = Some(*id);
                            break;
                        }
                    } else if crew_tab.name == tab.name {
                        // Already confirmed
                        found_tab_id = Some(*id);
                        break;
                    }
                }

                if let Some(tab_id) = found_tab_id {
                    current_tab_ids.insert(tab_id);
                } else {
                    // User-defined name - we don't know the tab_id, skip tracking
                    eprintln!("[crew:leader] Ignoring user-defined name '{}' (can't extract tab_id)", tab.name);
                }
            }
        }

        // Remove closed tabs
        let closed: Vec<u32> = self
            .known_tabs
            .keys()
            .filter(|tab_id| !current_tab_ids.contains(tab_id))
            .cloned()
            .collect();

        for tab_id in closed {
            if let Some(crew_tab) = self.known_tabs.remove(&tab_id) {
                if !crew_tab.user_defined {
                    eprintln!("[crew:leader] Tab #{} '{}' closed, name returns to pool", tab_id, crew_tab.name);
                } else {
                    eprintln!("[crew:leader] Tab #{} '{}' closed (user-defined, discarded)", tab_id, crew_tab.name);
                }
            }
        }
    }
}

// Legacy methods removed - see allocate_from_pool() in Leader State Management section

// ============================================================================
// Plugin Implementation
// ============================================================================

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.config = Config::from_btreemap(&configuration);

        eprintln!("[crew] load() config={:?}", configuration);

        // Don't call set_selectable(false) here - we need to remain selectable
        // so the user can focus this pane and grant permission on first run
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
        subscribe(&[
            EventType::TabUpdate,
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
                eprintln!("Got unrecognized event: {:?}", event);
            }
        }
        if self.tabs.is_empty() {
            should_render = false;
        }
        should_render
    }

    fn render(&mut self, rows: usize, cols: usize) {
        // Detect leader on first render: load_plugins instance gets permission dialog (rows > 1)
        // Tab-bar instances get rows=1 (or rows=0 briefly)
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

        // Renderer: show tab names as-is from TabUpdate (for now, no activity indicators yet)
        let names: Vec<String> = self.tabs
            .iter()
            .map(|tab| format!("{} [○]", tab.name))
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
// Tests (TODO: rewrite for leader/renderer architecture)
// ============================================================================

// #[cfg(test)]
// mod tests {
//     Tests temporarily disabled during leader/renderer refactor
// }
