use std::collections::{BTreeMap, HashSet};
use zellij_tile::prelude::*;

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

#[derive(Default)]
struct State {
    tabs: Vec<TabInfo>,
    active_tab_idx: usize,
    assigned_names: BTreeMap<u32, String>,
    last_assigned_idx: Option<usize>,
    config: Config,
}

impl State {
    fn is_default_name(name: &str) -> bool {
        if let Some(rest) = name.strip_prefix("Tab #") {
            rest.parse::<u32>().is_ok()
        } else {
            false
        }
    }

    fn get_used_names(&self) -> HashSet<&str> {
        self.assigned_names.values().map(|s| s.as_str()).collect()
    }

    fn allocate_name_round_robin(&mut self) -> Option<String> {
        if self.config.names.is_empty() {
            return None;
        }

        let used_names = self.get_used_names();
        let start_idx = self.last_assigned_idx.map(|i| i + 1).unwrap_or(0);
        let pool_len = self.config.names.len();

        // Try each name starting from after the last assigned
        for offset in 0..pool_len {
            let idx = (start_idx + offset) % pool_len;
            let name = &self.config.names[idx];
            if !used_names.contains(name.as_str()) {
                self.last_assigned_idx = Some(idx);
                return Some(name.clone());
            }
        }

        // Pool exhausted
        None
    }

    fn allocate_name_fill_in(&self) -> Option<String> {
        let used_names = self.get_used_names();

        // Find first unused name in pool order
        for name in &self.config.names {
            if !used_names.contains(name.as_str()) {
                return Some(name.clone());
            }
        }

        // Pool exhausted
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

    fn process_tabs(&mut self, tabs: Vec<TabInfo>) {
        // Track which tab IDs still exist
        let current_tab_ids: HashSet<u32> = tabs.iter().map(|t| t.position as u32).collect();

        // Remove assignments for tabs that no longer exist
        self.assigned_names
            .retain(|tab_id, _| current_tab_ids.contains(tab_id));

        // Find tabs that need naming
        let mut renames: Vec<(usize, String)> = Vec::new();

        for tab in &tabs {
            let tab_id = tab.position as u32;

            // Skip if already assigned
            if self.assigned_names.contains_key(&tab_id) {
                continue;
            }

            // Skip custom names if rename_custom is false
            if !self.config.rename_custom && !Self::is_default_name(&tab.name) {
                continue;
            }

            // Allocate a name
            if let Some(name) = self.allocate_name() {
                self.assigned_names.insert(tab_id, name.clone());
                let formatted = self.format_tab_name(&name, tab.position);
                renames.push((tab.position, formatted));
            }
        }

        // Update stored tabs
        self.tabs = tabs;

        // Execute renames
        for (position, new_name) in renames {
            rename_tab(position as u32 + 1, new_name);
        }
    }
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.config = Config::from_btreemap(&configuration);

        request_permission(&[PermissionType::ReadApplicationState]);
        subscribe(&[EventType::TabUpdate, EventType::PermissionRequestResult]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::TabUpdate(tabs) => {
                if let Some(active_idx) = tabs.iter().position(|t| t.active) {
                    self.active_tab_idx = active_idx;
                }
                self.process_tabs(tabs);
            }
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                set_selectable(false);
                subscribe(&[EventType::TabUpdate]);
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                eprintln!("zellij-crew: Permission denied - tab renaming disabled");
            }
            _ => {}
        }
        false // No rendering needed
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        // No UI rendering - this plugin only renames tabs
    }
}

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
        state.assigned_names.insert(0, "a".to_string());

        assert_eq!(state.allocate_name(), Some("b".to_string()));
        state.assigned_names.insert(1, "b".to_string());

        assert_eq!(state.allocate_name(), Some("c".to_string()));
        state.assigned_names.insert(2, "c".to_string());

        // Pool exhausted
        assert_eq!(state.allocate_name(), None);
    }

    #[test]
    fn test_round_robin_skips_freed_names() {
        let mut state = make_state(&["a", "b", "c", "d"], AllocationMode::RoundRobin);

        // Allocate a, b, c
        assert_eq!(state.allocate_name(), Some("a".to_string()));
        state.assigned_names.insert(0, "a".to_string());
        assert_eq!(state.allocate_name(), Some("b".to_string()));
        state.assigned_names.insert(1, "b".to_string());
        assert_eq!(state.allocate_name(), Some("c".to_string()));
        state.assigned_names.insert(2, "c".to_string());

        // Free "b"
        state.assigned_names.remove(&1);

        // Next allocation should be "d", not "b" (round-robin continues forward)
        assert_eq!(state.allocate_name(), Some("d".to_string()));
    }

    #[test]
    fn test_fill_in_reuses_freed_names() {
        let mut state = make_state(&["a", "b", "c", "d"], AllocationMode::FillIn);

        // Allocate a, b, c
        assert_eq!(state.allocate_name(), Some("a".to_string()));
        state.assigned_names.insert(0, "a".to_string());
        assert_eq!(state.allocate_name(), Some("b".to_string()));
        state.assigned_names.insert(1, "b".to_string());
        assert_eq!(state.allocate_name(), Some("c".to_string()));
        state.assigned_names.insert(2, "c".to_string());

        // Free "b"
        state.assigned_names.remove(&1);

        // Fill-in should reuse "b"
        assert_eq!(state.allocate_name(), Some("b".to_string()));
    }

    #[test]
    fn test_fill_in_fills_gaps_in_order() {
        let mut state = make_state(&["a", "b", "c", "d"], AllocationMode::FillIn);

        // Allocate a, b, c, d
        state.assigned_names.insert(0, "a".to_string());
        state.assigned_names.insert(1, "b".to_string());
        state.assigned_names.insert(2, "c".to_string());
        state.assigned_names.insert(3, "d".to_string());

        // Free "b" and "d"
        state.assigned_names.remove(&1);
        state.assigned_names.remove(&3);

        // Fill-in should return "b" first (earlier in pool order)
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
}
