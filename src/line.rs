// Line layout and LinePart utilities for tab-bar rendering

/// A styled segment of the tab line with its visual width.
/// Pairs ANSI-styled content with width for layout calculations.
#[derive(Debug, Clone, Default)]
pub struct LinePart {
    /// ANSI-styled text content
    pub part: String,
    /// Visual width in terminal columns (for layout)
    pub len: usize,
    /// Tab index for mouse click detection (None for non-tab elements)
    pub tab_index: Option<usize>,
}

/// Build the complete tab line, handling overflow when tabs don't fit.
/// Returns the line parts and total width.
pub fn build_tab_line(
    tabs: Vec<LinePart>,
    active_tab_idx: usize,
    max_width: usize,
) -> Vec<LinePart> {
    let total_width: usize = tabs.iter().map(|t| t.len).sum();

    if total_width <= max_width {
        // Everything fits
        return tabs;
    }

    // Need to collapse some tabs - always show active tab
    // Add tabs left/right of active alternately while they fit
    let mut result = Vec::new();
    let mut current_width = 0;
    let mut left_idx = active_tab_idx as isize - 1;
    let mut right_idx = active_tab_idx + 1;
    let mut hidden_left = 0;
    let mut hidden_right = 0;

    // Reserve space for collapse indicators
    let collapse_indicator_width = 6; // " <- +N" or "+N -> "
    let available = max_width.saturating_sub(collapse_indicator_width * 2);

    // Always include active tab
    if active_tab_idx < tabs.len() {
        let active = &tabs[active_tab_idx];
        if active.len <= available {
            result.push(active.clone());
            current_width = active.len;
        }
    }

    // Alternate adding tabs left and right
    loop {
        let mut added = false;

        // Try left
        if left_idx >= 0 {
            let idx = left_idx as usize;
            if current_width + tabs[idx].len <= available {
                result.insert(0, tabs[idx].clone());
                current_width += tabs[idx].len;
                left_idx -= 1;
                added = true;
            } else {
                hidden_left = (left_idx + 1) as usize;
                left_idx = -1;
            }
        }

        // Try right
        if right_idx < tabs.len() {
            if current_width + tabs[right_idx].len <= available {
                result.push(tabs[right_idx].clone());
                current_width += tabs[right_idx].len;
                right_idx += 1;
                added = true;
            } else {
                hidden_right = tabs.len() - right_idx;
                right_idx = tabs.len();
            }
        }

        if !added {
            break;
        }
    }

    // Count any remaining hidden tabs
    if left_idx >= 0 {
        hidden_left = (left_idx + 1) as usize;
    }
    if right_idx < tabs.len() {
        hidden_right = tabs.len() - right_idx;
    }

    // Add collapse indicators
    if hidden_left > 0 {
        let indicator = format!(" <-+{} ", hidden_left);
        let len = indicator.len();
        result.insert(
            0,
            LinePart {
                part: indicator,
                len,
                tab_index: Some(0), // Click jumps to first hidden tab
            },
        );
    }

    if hidden_right > 0 {
        let indicator = format!(" +{}-> ", hidden_right);
        let len = indicator.len();
        result.push(LinePart {
            part: indicator,
            len,
            tab_index: Some(tabs.len() - 1), // Click jumps to last tab
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_tab_line_fits() {
        let tabs = vec![
            LinePart { part: "tab1".to_string(), len: 4, tab_index: Some(0) },
            LinePart { part: "tab2".to_string(), len: 4, tab_index: Some(1) },
        ];
        let result = build_tab_line(tabs, 0, 100);
        assert_eq!(result.len(), 2);
    }
}
