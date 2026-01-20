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
    // Names from our pool currently assigned to tabs
    used_names: HashSet<String>,
    // For round-robin: index of last assigned name in pool
    last_assigned_idx: Option<usize>,
    // Pending rename: (position, expected_name) - ignore updates until confirmed
    pending_rename: Option<(usize, String)>,
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

    fn is_pool_name(&self, name: &str) -> bool {
        self.config.names.iter().any(|n| n == name)
    }

    fn allocate_name_round_robin(&mut self) -> Option<String> {
        if self.config.names.is_empty() {
            return None;
        }

        let start_idx = self.last_assigned_idx.map(|i| i + 1).unwrap_or(0);
        let pool_len = self.config.names.len();

        // Try each name starting from after the last assigned
        for offset in 0..pool_len {
            let idx = (start_idx + offset) % pool_len;
            let name = &self.config.names[idx];
            if !self.used_names.contains(name) {
                self.last_assigned_idx = Some(idx);
                return Some(name.clone());
            }
        }

        // Pool exhausted
        None
    }

    fn allocate_name_fill_in(&self) -> Option<String> {
        // Find first unused name in pool order
        for name in &self.config.names {
            if !self.used_names.contains(name) {
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
        // Debug: log incoming tabs with all relevant fields
        let tab_info: Vec<_> = tabs.iter().map(|t| format!("pos{}:'{}'", t.position, &t.name)).collect();
        eprintln!("zellij-crew: TabUpdate tabs={:?} pending={:?}", tab_info, self.pending_rename);
        // Log full TabInfo to see what fields are available
        if !tabs.is_empty() {
            eprintln!("zellij-crew: First tab debug: {:?}", tabs[0]);
        }

        // If we have a pending rename, wait for it to be confirmed before doing anything else
        if let Some((pos, expected_name)) = &self.pending_rename {
            if let Some(tab) = tabs.iter().find(|t| t.position == *pos) {
                if &tab.name == expected_name {
                    // Rename confirmed!
                    eprintln!("zellij-crew: pending rename confirmed for pos {} = '{}'", pos, expected_name);
                    self.pending_rename = None;
                } else {
                    // Still waiting for rename to take effect
                    eprintln!("zellij-crew: waiting for pending rename pos {} (have '{}', want '{}')", pos, &tab.name, expected_name);
                    return;
                }
            } else {
                // Tab at that position no longer exists, clear pending
                eprintln!("zellij-crew: pending rename pos {} no longer exists, clearing", pos);
                self.pending_rename = None;
            }
        }

        // First pass: update used_names based on current tab names
        let mut current_pool_names: HashSet<String> = HashSet::new();
        for tab in &tabs {
            if self.is_pool_name(&tab.name) {
                current_pool_names.insert(tab.name.clone());
            }
        }
        self.used_names = current_pool_names.clone();
        eprintln!("zellij-crew: used_names={:?}", current_pool_names);

        // Second pass: find ONE tab needing a name and rename it
        for tab in &tabs {
            if self.is_pool_name(&tab.name) {
                continue;
            }
            if !self.config.rename_custom && !Self::is_default_name(&tab.name) {
                continue;
            }

            // Parse the stable tab index from "Tab #N" - this is what rename_tab expects
            // (rename_tab uses index, not position, but TabInfo only gives us position)
            let tab_index = match tab.name.strip_prefix("Tab #").and_then(|s| s.parse::<u32>().ok()) {
                Some(n) => n,
                None => {
                    // Can't safely rename - we don't know the stable index
                    eprintln!("zellij-crew: can't rename '{}' at pos {} - no stable index", tab.name, tab.position);
                    continue;
                }
            };

            if let Some(name) = self.allocate_name() {
                self.used_names.insert(name.clone());
                let formatted = self.format_tab_name(&name, tab.position);
                eprintln!("zellij-crew: renaming '{}' at pos {} to '{}' (rename_tab({}))", tab.name, tab.position, &formatted, tab_index);
                self.pending_rename = Some((tab.position, formatted.clone()));
                rename_tab(tab_index, formatted);
                return;
            }
        }
        eprintln!("zellij-crew: no tabs need renaming");
    }
}

#[cfg(target_family = "wasm")]
register_plugin!(State);

#[cfg(target_family = "wasm")]
impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.config = Config::from_btreemap(&configuration);

        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
        subscribe(&[EventType::TabUpdate, EventType::PermissionRequestResult]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::TabUpdate(tabs) => {
                self.process_tabs(tabs);
            }
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                // Re-subscribe after permission granted
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
        state.used_names.insert("a".to_string());

        assert_eq!(state.allocate_name(), Some("b".to_string()));
        state.used_names.insert("b".to_string());

        assert_eq!(state.allocate_name(), Some("c".to_string()));
        state.used_names.insert("c".to_string());

        // Pool exhausted
        assert_eq!(state.allocate_name(), None);
    }

    #[test]
    fn test_round_robin_skips_used_names() {
        let mut state = make_state(&["a", "b", "c", "d"], AllocationMode::RoundRobin);

        // Allocate a, b, c
        assert_eq!(state.allocate_name(), Some("a".to_string()));
        state.used_names.insert("a".to_string());
        assert_eq!(state.allocate_name(), Some("b".to_string()));
        state.used_names.insert("b".to_string());
        assert_eq!(state.allocate_name(), Some("c".to_string()));
        state.used_names.insert("c".to_string());

        // Free "b"
        state.used_names.remove("b");

        // Next allocation should be "d", not "b" (round-robin continues forward)
        assert_eq!(state.allocate_name(), Some("d".to_string()));
    }

    #[test]
    fn test_fill_in_reuses_freed_names() {
        let mut state = make_state(&["a", "b", "c", "d"], AllocationMode::FillIn);

        // Allocate a, b, c
        assert_eq!(state.allocate_name(), Some("a".to_string()));
        state.used_names.insert("a".to_string());
        assert_eq!(state.allocate_name(), Some("b".to_string()));
        state.used_names.insert("b".to_string());
        assert_eq!(state.allocate_name(), Some("c".to_string()));
        state.used_names.insert("c".to_string());

        // Free "b"
        state.used_names.remove("b");

        // Fill-in should reuse "b"
        assert_eq!(state.allocate_name(), Some("b".to_string()));
    }

    #[test]
    fn test_fill_in_fills_gaps_in_order() {
        let mut state = make_state(&["a", "b", "c", "d"], AllocationMode::FillIn);

        // Mark a, b, c, d as used
        state.used_names.insert("a".to_string());
        state.used_names.insert("b".to_string());
        state.used_names.insert("c".to_string());
        state.used_names.insert("d".to_string());

        // Free "b" and "d"
        state.used_names.remove("b");
        state.used_names.remove("d");

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

    #[test]
    fn test_is_pool_name() {
        let state = make_state(&["alice", "bob", "carol"], AllocationMode::FillIn);
        assert!(state.is_pool_name("alice"));
        assert!(state.is_pool_name("bob"));
        assert!(!state.is_pool_name("Tab #1"));
        assert!(!state.is_pool_name("mywork"));
    }
}
