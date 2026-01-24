// zellij-crew: Tab-bar plugin with named tabs and activity indicators

mod line;
mod tab;

use std::collections::{BTreeMap, HashMap, HashSet};
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

        Config {
            names,
            mode,
            show_position,
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

    // Cached tab line for mouse click detection
    tab_line: Vec<LinePart>,

    // Name allocation: tab position -> assigned name
    assigned_names: HashMap<usize, String>,
    last_assigned_idx: Option<usize>,

    // Configuration
    config: Config,
}

// ============================================================================
// Name Allocation
// ============================================================================

impl State {
    fn used_names(&self) -> HashSet<String> {
        self.assigned_names.values().cloned().collect()
    }

    fn allocate_name_round_robin(&mut self) -> Option<String> {
        if self.config.names.is_empty() {
            return None;
        }

        let used = self.used_names();
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

    fn allocate_name_fill_in(&self) -> Option<String> {
        let used = self.used_names();
        for name in &self.config.names {
            if !used.contains(name) {
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

    fn format_display_name(&self, name: &str, position: usize) -> String {
        if self.config.show_position {
            format!("{} <{}>", name, position + 1)
        } else {
            name.to_string()
        }
    }

    /// Get or allocate display name for a tab
    fn get_display_name(&mut self, position: usize) -> String {
        // Already assigned?
        if let Some(name) = self.assigned_names.get(&position) {
            return self.format_display_name(name, position);
        }

        // Allocate new name
        if let Some(name) = self.allocate_name() {
            self.assigned_names.insert(position, name.clone());
            return self.format_display_name(&name, position);
        }

        // Pool exhausted - use position number
        format!("Tab {}", position + 1)
    }

    /// Clean up names for tabs that no longer exist
    fn cleanup_names(&mut self, current_positions: &HashSet<usize>) {
        self.assigned_names
            .retain(|pos, _| current_positions.contains(pos));
    }
}

// ============================================================================
// Tab Update Handling
// ============================================================================

impl State {
    fn handle_tab_update(&mut self, tabs: Vec<TabInfo>) {
        // Find active tab
        self.active_tab_idx = tabs.iter().position(|t| t.active).unwrap_or(0);

        // Clean up names for closed tabs
        let current_positions: HashSet<usize> = tabs.iter().map(|t| t.position).collect();
        self.cleanup_names(&current_positions);

        // Store tabs
        self.tabs = tabs;
    }
}

// ============================================================================
// Mouse Handling
// ============================================================================

impl State {
    fn handle_mouse_click(&self, x: usize) {
        let mut current_x = 0;
        for part in &self.tab_line {
            if x >= current_x && x < current_x + part.len {
                if let Some(tab_idx) = part.tab_index {
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

        // Collect tab info to avoid borrow issues
        let tab_data: Vec<(usize, usize, bool)> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(idx, tab)| (idx, tab.position, idx == self.active_tab_idx))
            .collect();

        // Build LinePart for each tab
        let mut tab_parts: Vec<LinePart> = Vec::new();
        for (idx, position, is_active) in tab_data {
            let display_name = self.get_display_name(position);

            // Add placeholder indicator
            let display_with_indicator = format!("{} [○]", display_name);

            let part = render_tab(&self.tabs[idx], is_active, &display_with_indicator);
            tab_parts.push(part);
        }

        // Handle overflow
        let tab_line = build_tab_line(tab_parts, self.active_tab_idx, cols);

        // Output the line
        let mut output = String::new();
        for part in &tab_line {
            output.push_str(&part.part);
        }

        // Fill remaining width with background
        let used_width: usize = tab_line.iter().map(|p| p.len).sum();
        if used_width < cols {
            let fill = " ".repeat(cols - used_width);
            let fill_text = zellij_tile::ui_components::Text::new(&fill);
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
            Event::ModeUpdate(_mode_info) => {
                true // re-render on mode change
            }
            Event::TabUpdate(tabs) => {
                self.handle_tab_update(tabs);
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
                eprintln!("zellij-crew: Permission denied");
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
        }
    }

    fn make_state(names: &[&str], mode: AllocationMode) -> State {
        State {
            config: make_config(names, mode),
            ..Default::default()
        }
    }

    #[test]
    fn test_config_defaults() {
        let config = Config::from_btreemap(&BTreeMap::new());
        assert_eq!(config.mode, AllocationMode::RoundRobin);
        assert!(!config.show_position);
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
        state.assigned_names.insert(0, "a".to_string());

        assert_eq!(state.allocate_name(), Some("b".to_string()));
        state.assigned_names.insert(1, "b".to_string());

        assert_eq!(state.allocate_name(), Some("c".to_string()));
        state.assigned_names.insert(2, "c".to_string());

        assert_eq!(state.allocate_name(), None);
    }

    #[test]
    fn test_fill_in_reuses_freed_names() {
        let mut state = make_state(&["a", "b", "c", "d"], AllocationMode::FillIn);

        state.assigned_names.insert(0, "a".to_string());
        state.assigned_names.insert(1, "b".to_string());
        state.assigned_names.insert(2, "c".to_string());

        // Simulate tab 1 closing
        state.assigned_names.remove(&1);

        // Fill-in should reuse "b"
        assert_eq!(state.allocate_name(), Some("b".to_string()));
    }

    #[test]
    fn test_cleanup_names() {
        let mut state = make_state(&["a", "b", "c"], AllocationMode::FillIn);
        state.assigned_names.insert(0, "a".to_string());
        state.assigned_names.insert(1, "b".to_string());
        state.assigned_names.insert(2, "c".to_string());

        // Tabs 0 and 2 remain, tab 1 closed
        let current: HashSet<usize> = [0, 2].iter().cloned().collect();
        state.cleanup_names(&current);

        assert!(state.assigned_names.contains_key(&0));
        assert!(!state.assigned_names.contains_key(&1));
        assert!(state.assigned_names.contains_key(&2));
    }

    #[test]
    fn test_format_display_name_without_position() {
        let state = make_state(&["a"], AllocationMode::RoundRobin);
        assert_eq!(state.format_display_name("alpha", 0), "alpha");
        assert_eq!(state.format_display_name("alpha", 5), "alpha");
    }

    #[test]
    fn test_format_display_name_with_position() {
        let mut state = make_state(&["a"], AllocationMode::RoundRobin);
        state.config.show_position = true;
        assert_eq!(state.format_display_name("alpha", 0), "alpha <1>");
        assert_eq!(state.format_display_name("alpha", 5), "alpha <6>");
    }

    #[test]
    fn test_empty_pool() {
        let mut state = make_state(&[], AllocationMode::RoundRobin);
        assert_eq!(state.allocate_name(), None);

        state.config.mode = AllocationMode::FillIn;
        assert_eq!(state.allocate_name(), None);
    }

    #[test]
    fn test_get_display_name_allocates() {
        let mut state = make_state(&["alice", "bob"], AllocationMode::FillIn);

        let name0 = state.get_display_name(0);
        assert_eq!(name0, "alice");
        assert!(state.assigned_names.contains_key(&0));

        let name1 = state.get_display_name(1);
        assert_eq!(name1, "bob");

        // Same position returns same name
        let name0_again = state.get_display_name(0);
        assert_eq!(name0_again, "alice");
    }

    #[test]
    fn test_pool_exhausted_fallback() {
        let mut state = make_state(&["a"], AllocationMode::FillIn);

        let name0 = state.get_display_name(0);
        assert_eq!(name0, "a");

        // Pool exhausted
        let name1 = state.get_display_name(1);
        assert_eq!(name1, "Tab 2");
    }
}
