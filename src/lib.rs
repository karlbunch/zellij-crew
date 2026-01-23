// zellij-crew: Tab naming and tab-bar plugin

mod line;
mod tab;

use std::collections::{BTreeMap, HashSet};
use zellij_tile::prelude::*;

use crate::line::{build_tab_line, LinePart};
use crate::tab::render_tab;

const DEFAULT_NAMES: &str = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey xray yankee zulu";

// ============================================================================
// Configuration
// ============================================================================

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
    rename_custom: bool,
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

        let rename_custom = config
            .get("rename_custom")
            .map(|s| s == "true")
            .unwrap_or(false);

        Config {
            names,
            mode,
            show_position,
            rename_custom,
        }
    }
}

// ============================================================================
// Plugin State
// ============================================================================

#[derive(Default)]
struct State {
    // Tab information from TabUpdate
    tabs: Vec<TabInfo>,
    active_tab_idx: usize,

    // Mode and styling info from ModeUpdate
    mode_info: Option<ModeInfo>,

    // Cached tab line for mouse click detection
    tab_line: Vec<LinePart>,

    // Name allocation state
    used_names: HashSet<String>,
    last_assigned_idx: Option<usize>,

    // Pending rename: (position, expected_name)
    pending_rename: Option<(usize, String)>,

    // Configuration
    config: Config,
}

// ============================================================================
// Name Allocation
// ============================================================================

impl State {
    fn is_default_name(name: &str) -> bool {
        if let Some(rest) = name.strip_prefix("Tab #") {
            rest.parse::<u32>().is_ok()
        } else {
            false
        }
    }

    fn is_pool_name(&self, name: &str) -> bool {
        self.config.names.iter().any(|n| n == name)
    }

    fn allocate_name_round_robin(&mut self) -> Option<String> {
        if self.config.names.is_empty() {
            return None;
        }

        let start_idx = self.last_assigned_idx.map(|i| i + 1).unwrap_or(0);
        let pool_len = self.config.names.len();

        for offset in 0..pool_len {
            let idx = (start_idx + offset) % pool_len;
            let name = &self.config.names[idx];
            if !self.used_names.contains(name) {
                self.last_assigned_idx = Some(idx);
                return Some(name.clone());
            }
        }

        None
    }

    fn allocate_name_fill_in(&self) -> Option<String> {
        for name in &self.config.names {
            if !self.used_names.contains(name) {
                return Some(name.clone());
            }
        }

        None
    }

    fn allocate_name(&mut self) -> Option<String> {
        match self.config.mode {
            AllocationMode::RoundRobin => self.allocate_name_round_robin(),
            AllocationMode::FillIn => self.allocate_name_fill_in(),
        }
    }

    fn format_tab_name(&self, name: &str, position: usize) -> String {
        if self.config.show_position {
            format!("{} <{}>", name, position + 1)
        } else {
            name.to_string()
        }
    }

    /// Get the display name for a tab (either assigned name or original)
    fn get_display_name(&self, tab: &TabInfo) -> String {
        // If tab already has a pool name, use it
        if self.is_pool_name(&tab.name) {
            return tab.name.clone();
        }
        // Otherwise use original name
        tab.name.clone()
    }
}

// ============================================================================
// Tab Processing (for background renaming mode)
// ============================================================================

impl State {
    fn process_tabs(&mut self, tabs: Vec<TabInfo>) {
        let tab_info: Vec<_> = tabs
            .iter()
            .map(|t| format!("pos{}:'{}'", t.position, &t.name))
            .collect();
        eprintln!(
            "zellij-crew: TabUpdate tabs={:?} pending={:?}",
            tab_info, self.pending_rename
        );

        // Handle pending rename confirmation
        if let Some((pos, expected_name)) = &self.pending_rename {
            if let Some(tab) = tabs.iter().find(|t| t.position == *pos) {
                if &tab.name == expected_name {
                    eprintln!(
                        "zellij-crew: pending rename confirmed for pos {} = '{}'",
                        pos, expected_name
                    );
                    self.pending_rename = None;
                } else {
                    eprintln!(
                        "zellij-crew: waiting for pending rename pos {} (have '{}', want '{}')",
                        pos, &tab.name, expected_name
                    );
                    return;
                }
            } else {
                eprintln!(
                    "zellij-crew: pending rename pos {} no longer exists, clearing",
                    pos
                );
                self.pending_rename = None;
            }
        }

        // Update used_names based on current tab names
        let mut current_pool_names: HashSet<String> = HashSet::new();
        for tab in &tabs {
            if self.is_pool_name(&tab.name) {
                current_pool_names.insert(tab.name.clone());
            }
        }
        self.used_names = current_pool_names.clone();

        // Find active tab
        self.active_tab_idx = tabs
            .iter()
            .position(|t| t.active)
            .unwrap_or(0);

        // Store tabs for rendering
        self.tabs = tabs.clone();

        // Find ONE tab needing a name and rename it
        for tab in &tabs {
            if self.is_pool_name(&tab.name) {
                continue;
            }
            if !self.config.rename_custom && !Self::is_default_name(&tab.name) {
                continue;
            }

            let tab_index = match tab.name.strip_prefix("Tab #").and_then(|s| s.parse::<u32>().ok())
            {
                Some(n) => n,
                None => {
                    eprintln!(
                        "zellij-crew: can't rename '{}' at pos {} - no stable index",
                        tab.name, tab.position
                    );
                    continue;
                }
            };

            if let Some(name) = self.allocate_name() {
                self.used_names.insert(name.clone());
                let formatted = self.format_tab_name(&name, tab.position);
                eprintln!(
                    "zellij-crew: renaming '{}' at pos {} to '{}' (rename_tab({}))",
                    tab.name, tab.position, &formatted, tab_index
                );
                self.pending_rename = Some((tab.position, formatted.clone()));
                rename_tab(tab_index, formatted);
                return;
            }
        }
    }
}

// ============================================================================
// Mouse Handling
// ============================================================================

impl State {
    fn handle_mouse_click(&self, x: usize) {
        // Find which tab was clicked based on cached tab_line
        let mut current_x = 0;
        for part in &self.tab_line {
            if x >= current_x && x < current_x + part.len {
                if let Some(tab_idx) = part.tab_index {
                    // Switch to this tab
                    go_to_tab(tab_idx as u32 + 1);
                    return;
                }
            }
            current_x += part.len;
        }
    }

    fn handle_scroll(&self, up: bool) {
        if up {
            go_to_previous_tab();
        } else {
            go_to_next_tab();
        }
    }
}

// ============================================================================
// Rendering
// ============================================================================

impl State {
    fn render_tab_bar(&mut self, cols: usize) {
        if self.tabs.is_empty() {
            return;
        }

        // Build LinePart for each tab
        let mut tab_parts: Vec<LinePart> = Vec::new();
        for (idx, tab) in self.tabs.iter().enumerate() {
            let is_active = idx == self.active_tab_idx;
            let display_name = self.get_display_name(tab);
            let part = render_tab(tab, is_active, &display_name);
            tab_parts.push(part);
        }

        // Handle overflow
        let tab_line = build_tab_line(tab_parts, self.active_tab_idx, cols);

        // Output the line
        let mut output = String::new();
        for part in &tab_line {
            output.push_str(&part.part);
        }

        // Fill remaining width with empty ribbon
        let used_width: usize = tab_line.iter().map(|p| p.len).sum();
        if used_width < cols {
            let fill = " ".repeat(cols - used_width);
            let fill_text = Text::new(&fill);
            output.push_str(&zellij_tile::ui_components::serialize_ribbon(&fill_text));
        }

        print!("{}", output);

        // Cache for mouse handling
        self.tab_line = tab_line;
    }
}

// ============================================================================
// Plugin Implementation
// ============================================================================

#[cfg(target_family = "wasm")]
register_plugin!(State);

#[cfg(target_family = "wasm")]
impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.config = Config::from_btreemap(&configuration);

        // Tab bar should not steal keyboard focus
        set_selectable(false);

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
        match event {
            Event::ModeUpdate(mode_info) => {
                self.mode_info = Some(mode_info);
                true // re-render
            }
            Event::TabUpdate(tabs) => {
                self.process_tabs(tabs);
                true // re-render
            }
            Event::Mouse(mouse_event) => {
                match mouse_event {
                    Mouse::LeftClick(_, x) => self.handle_mouse_click(x),
                    Mouse::ScrollUp(_) => self.handle_scroll(true),
                    Mouse::ScrollDown(_) => self.handle_scroll(false),
                    _ => {}
                }
                false // no re-render needed for mouse
            }
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                subscribe(&[EventType::TabUpdate, EventType::ModeUpdate, EventType::Mouse]);
                false
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                eprintln!("zellij-crew: Permission denied - plugin disabled");
                false
            }
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, cols: usize) {
        self.render_tab_bar(cols);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(names: &[&str], mode: AllocationMode) -> Config {
        Config {
            names: names.iter().map(|s| s.to_string()).collect(),
            mode,
            show_position: false,
            rename_custom: false,
        }
    }

    fn make_state(names: &[&str], mode: AllocationMode) -> State {
        State {
            config: make_config(names, mode),
            ..Default::default()
        }
    }

    #[test]
    fn test_is_default_name() {
        assert!(State::is_default_name("Tab #1"));
        assert!(State::is_default_name("Tab #42"));
        assert!(State::is_default_name("Tab #999"));
        assert!(!State::is_default_name("Tab #"));
        assert!(!State::is_default_name("Tab #abc"));
        assert!(!State::is_default_name("mywork"));
        assert!(!State::is_default_name("alpha"));
        assert!(!State::is_default_name(""));
    }

    #[test]
    fn test_config_defaults() {
        let config = Config::from_btreemap(&BTreeMap::new());
        assert_eq!(config.mode, AllocationMode::RoundRobin);
        assert!(!config.show_position);
        assert!(!config.rename_custom);
        assert!(!config.names.is_empty());
        assert_eq!(config.names[0], "alpha");
    }

    #[test]
    fn test_config_custom_names() {
        let mut map = BTreeMap::new();
        map.insert("names".to_string(), "a b c".to_string());
        let config = Config::from_btreemap(&map);
        assert_eq!(config.names, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_config_fill_in_mode() {
        let mut map = BTreeMap::new();
        map.insert("mode".to_string(), "fill-in".to_string());
        let config = Config::from_btreemap(&map);
        assert_eq!(config.mode, AllocationMode::FillIn);
    }

    #[test]
    fn test_round_robin_sequential() {
        let mut state = make_state(&["a", "b", "c"], AllocationMode::RoundRobin);

        assert_eq!(state.allocate_name(), Some("a".to_string()));
        state.used_names.insert("a".to_string());

        assert_eq!(state.allocate_name(), Some("b".to_string()));
        state.used_names.insert("b".to_string());

        assert_eq!(state.allocate_name(), Some("c".to_string()));
        state.used_names.insert("c".to_string());

        assert_eq!(state.allocate_name(), None);
    }

    #[test]
    fn test_round_robin_skips_used_names() {
        let mut state = make_state(&["a", "b", "c", "d"], AllocationMode::RoundRobin);

        assert_eq!(state.allocate_name(), Some("a".to_string()));
        state.used_names.insert("a".to_string());
        assert_eq!(state.allocate_name(), Some("b".to_string()));
        state.used_names.insert("b".to_string());
        assert_eq!(state.allocate_name(), Some("c".to_string()));
        state.used_names.insert("c".to_string());

        state.used_names.remove("b");

        assert_eq!(state.allocate_name(), Some("d".to_string()));
    }

    #[test]
    fn test_fill_in_reuses_freed_names() {
        let mut state = make_state(&["a", "b", "c", "d"], AllocationMode::FillIn);

        assert_eq!(state.allocate_name(), Some("a".to_string()));
        state.used_names.insert("a".to_string());
        assert_eq!(state.allocate_name(), Some("b".to_string()));
        state.used_names.insert("b".to_string());
        assert_eq!(state.allocate_name(), Some("c".to_string()));
        state.used_names.insert("c".to_string());

        state.used_names.remove("b");

        assert_eq!(state.allocate_name(), Some("b".to_string()));
    }

    #[test]
    fn test_fill_in_fills_gaps_in_order() {
        let mut state = make_state(&["a", "b", "c", "d"], AllocationMode::FillIn);

        state.used_names.insert("a".to_string());
        state.used_names.insert("b".to_string());
        state.used_names.insert("c".to_string());
        state.used_names.insert("d".to_string());

        state.used_names.remove("b");
        state.used_names.remove("d");

        assert_eq!(state.allocate_name(), Some("b".to_string()));
    }

    #[test]
    fn test_format_tab_name_without_position() {
        let state = make_state(&["a"], AllocationMode::RoundRobin);
        assert_eq!(state.format_tab_name("alpha", 0), "alpha");
        assert_eq!(state.format_tab_name("alpha", 5), "alpha");
    }

    #[test]
    fn test_format_tab_name_with_position() {
        let mut state = make_state(&["a"], AllocationMode::RoundRobin);
        state.config.show_position = true;
        assert_eq!(state.format_tab_name("alpha", 0), "alpha <1>");
        assert_eq!(state.format_tab_name("alpha", 5), "alpha <6>");
    }

    #[test]
    fn test_empty_pool() {
        let mut state = make_state(&[], AllocationMode::RoundRobin);
        assert_eq!(state.allocate_name(), None);

        state.config.mode = AllocationMode::FillIn;
        assert_eq!(state.allocate_name(), None);
    }

    #[test]
    fn test_is_pool_name() {
        let state = make_state(&["alice", "bob", "carol"], AllocationMode::FillIn);
        assert!(state.is_pool_name("alice"));
        assert!(state.is_pool_name("bob"));
        assert!(!state.is_pool_name("Tab #1"));
        assert!(!state.is_pool_name("mywork"));
    }
}
