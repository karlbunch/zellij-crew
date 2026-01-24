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

#[derive(Default)]
struct State {
    tabs: Vec<TabInfo>,
    active_tab_idx: usize,
    mode_info: ModeInfo,
    tab_line: Vec<LinePart>,

    // Name allocation
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

    fn get_display_name(&mut self, position: usize) -> String {
        if let Some(name) = self.assigned_names.get(&position) {
            return self.format_display_name(name, position);
        }

        if let Some(name) = self.allocate_name() {
            self.assigned_names.insert(position, name.clone());
            return self.format_display_name(&name, position);
        }

        format!("Tab {}", position + 1)
    }

    fn cleanup_names(&mut self, current_positions: &HashSet<usize>) {
        self.assigned_names
            .retain(|pos, _| current_positions.contains(pos));
    }
}

// ============================================================================
// Plugin Implementation
// ============================================================================

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.config = Config::from_btreemap(&configuration);

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
        let mut should_render = false;
        match event {
            Event::ModeUpdate(mode_info) => {
                if self.mode_info != mode_info {
                    should_render = true;
                }
                self.mode_info = mode_info;
            }
            Event::TabUpdate(tabs) => {
                if let Some(active_tab_index) = tabs.iter().position(|t| t.active) {
                    let active_tab_idx = active_tab_index + 1;

                    if self.active_tab_idx != active_tab_idx || self.tabs != tabs {
                        should_render = true;
                    }
                    self.active_tab_idx = active_tab_idx;

                    // Clean up names for closed tabs
                    let current_positions: HashSet<usize> =
                        tabs.iter().map(|t| t.position).collect();
                    self.cleanup_names(&current_positions);

                    self.tabs = tabs;
                } else {
                    eprintln!("Could not find active tab.");
                }
            }
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                set_selectable(false);
                subscribe(&[
                    EventType::TabUpdate,
                    EventType::ModeUpdate,
                    EventType::Mouse,
                ]);
                should_render = true;
            }
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                eprintln!("Permission denied - tab bar will not function properly");
            }
            Event::Mouse(me) => match me {
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
            },
            _ => {
                eprintln!("Got unrecognized event: {:?}", event);
            }
        }
        if self.tabs.is_empty() {
            should_render = false;
        }
        should_render
    }

    fn render(&mut self, _rows: usize, cols: usize) {
        if self.tabs.is_empty() {
            // Don't render anything - let zellij show its permission dialog cleanly
            return;
        }

        // Collect positions first to avoid borrow issues
        let tab_positions: Vec<usize> = self.tabs.iter().map(|t| t.position).collect();

        // Pre-allocate names
        let names: Vec<String> = tab_positions
            .iter()
            .map(|&pos| {
                let base = self.get_display_name(pos);
                format!("{} [○]", base)
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
                None,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(names: &[&str], mode: AllocationMode) -> Config {
        Config {
            names: names.iter().map(|s| s.to_string()).collect(),
            mode,
            show_position: false,
            hide_swap_layout_indication: false,
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

        state.assigned_names.remove(&1);

        assert_eq!(state.allocate_name(), Some("b".to_string()));
    }

    #[test]
    fn test_cleanup_names() {
        let mut state = make_state(&["a", "b", "c"], AllocationMode::FillIn);
        state.assigned_names.insert(0, "a".to_string());
        state.assigned_names.insert(1, "b".to_string());
        state.assigned_names.insert(2, "c".to_string());

        let current: HashSet<usize> = [0, 2].iter().cloned().collect();
        state.cleanup_names(&current);

        assert!(state.assigned_names.contains_key(&0));
        assert!(!state.assigned_names.contains_key(&1));
        assert!(state.assigned_names.contains_key(&2));
    }
}
