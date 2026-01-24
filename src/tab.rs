// Individual tab rendering using zellij's Text/ribbon API

use crate::line::LinePart;
use unicode_width::UnicodeWidthStr;
use zellij_tile::prelude::*;
use zellij_tile::ui_components::{serialize_ribbon, Text};

/// Render a single tab as a LinePart using zellij's Text API
pub fn render_tab(
    tab: &TabInfo,
    is_active: bool,
    display_name: &str,
) -> LinePart {
    // Build tab text with padding
    let tab_text = format!(" {} ", display_name);
    let tab_text_len = UnicodeWidthStr::width(tab_text.as_str());

    // Create Text with appropriate styling
    let text = if is_active {
        Text::new(&tab_text).selected()
    } else {
        Text::new(&tab_text)
    };

    // Serialize to zellij ribbon format
    let part = serialize_ribbon(&text);

    LinePart {
        part,
        len: tab_text_len,
        tab_index: Some(tab.position),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_tab_info(position: usize, name: &str, active: bool) -> TabInfo {
        TabInfo {
            position,
            name: name.to_string(),
            active,
            panes_to_hide: 0,
            is_fullscreen_active: false,
            is_sync_panes_active: false,
            are_floating_panes_visible: false,
            other_focused_clients: vec![],
            active_swap_layout_name: None,
            is_swap_layout_dirty: false,
            viewport_rows: 24,
            viewport_columns: 80,
            display_area_columns: 80,
            display_area_rows: 24,
            selectable_floating_panes_count: 0,
            selectable_tiled_panes_count: 0,
        }
    }

    #[test]
    fn test_render_tab_basic() {
        let tab = mock_tab_info(0, "alpha", false);
        let part = render_tab(&tab, false, "alpha");

        assert!(part.len > 0);
        assert_eq!(part.tab_index, Some(0));
    }

    #[test]
    fn test_render_tab_active() {
        let tab = mock_tab_info(1, "bravo", true);
        let part = render_tab(&tab, true, "bravo");

        assert!(part.len > 0);
        assert_eq!(part.tab_index, Some(1));
    }
}
