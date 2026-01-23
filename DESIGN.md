# zellij-crew Design Document

A zellij plugin that automatically renames tabs using names from a configurable pool.

## Purpose

Replace generic "Tab #1", "Tab #2" names with memorable names from a pool (e.g., phonetic alphabet, Star Wars characters, crew names). This makes it easier to reference tabs verbally or mentally track which tab is which.

### Vision: Addressable Agent Identities

The broader goal is enabling communication between AI agents running in different tabs. With named tabs, you can say "Ask Bob about the API schema" and the agent in your current tab can address the agent in the "Bob" tab by name. Tab names become addressable identities for inter-agent communication.

This plugin handles the naming layer. Future work may add:
- Message passing between tabs
- Agent discovery ("who's available?")
- Context sharing between named agents

## Configuration

Configuration is passed via zellij's plugin configuration block:

```kdl
plugins {
    crew location="file:target/wasm32-wasip1/release/zellij-crew.wasm" {
        names "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey xray yankee zulu"
        mode "round-robin"      // or "fill-in"
        show_position "false"   // or "true" for "alpha <1>"
        rename_custom "false"   // or "true" to rename tabs with custom names
    }
}
```

### Configuration Options

| Option | Values | Default | Description |
|--------|--------|---------|-------------|
| `names` | space-separated string | NATO phonetic | Pool of names to assign to tabs |
| `mode` | `round-robin`, `fill-in` | `round-robin` | Name allocation strategy |
| `show_position` | `true`, `false` | `false` | Append tab position like "alpha <1>" |
| `rename_custom` | `true`, `false` | `false` | Whether to rename tabs that already have custom names |

### Allocation Modes

**round-robin**: Assigns names sequentially in pool order. When a new tab is created, it gets the next name after the most recently assigned name. Cycles back to the start when the pool is exhausted.

Example with pool [a, b, c, d]:
```
Tab created → "a"
Tab created → "b"
Tab created → "c"
Close "b", new tab created → "d"
New tab created → pool exhausted, left unnamed
```

**fill-in**: Assigns the first available (unused) name from the pool. Names are allocated to fill gaps.

Example with pool [a, b, c, d]:
```
Tab created → "a"
Tab created → "b"
Tab created → "c"
Close "b", new tab created → "b" (fills the gap)
```

## State Management

```rust
struct State {
    // Tab tracking
    tabs: Vec<TabInfo>,           // Current tabs from TabUpdate
    active_tab_idx: usize,        // Currently focused tab

    // Name allocation
    assigned_names: HashMap<u32, String>,  // tab_id -> assigned name
    last_assigned_idx: usize,     // For round-robin mode: index of last assigned name

    // Configuration
    config: Config,
}

struct Config {
    names: Vec<String>,           // Name pool
    mode: AllocationMode,         // round-robin or fill-in
    show_position: bool,          // Whether to show position numbers
    rename_custom: bool,          // Whether to rename custom-named tabs
}

enum AllocationMode {
    RoundRobin,
    FillIn,
}
```

## Event Flow

### Plugin Lifecycle

1. **load()**: Parse configuration from `BTreeMap<String, String>`, initialize state
2. **update()**: Handle events (primarily `TabUpdate`)
3. **render()**: Not used for tab renaming (we use actions, not UI)

### Tab Rename Flow

```
TabUpdate event received
    │
    ├─► For each tab in update:
    │       │
    │       ├─► Tab already has assigned name?
    │       │       YES → Skip (unless show_position changed)
    │       │       NO  → Continue
    │       │
    │       ├─► Tab has custom name AND !rename_custom?
    │       │       YES → Skip
    │       │       NO  → Continue
    │       │
    │       └─► Allocate name based on mode
    │               │
    │               ├─► round-robin: names[(last_assigned_idx + 1) % len]
    │               └─► fill-in: first name not in assigned_names.values()
    │
    └─► For each newly assigned name:
            │
            └─► Send RenameTab action with formatted name
```

### Detecting "Custom" vs "Default" Tab Names

A tab has a default name if it matches the pattern `Tab #N` where N is a number. All other names are considered custom (user-set).

```rust
fn is_default_name(name: &str) -> bool {
    name.starts_with("Tab #") && name[5..].parse::<u32>().is_ok()
}
```

## Actions

The plugin uses zellij's action system to rename tabs:

```rust
// Rename the tab at a specific index
rename_tab(tab_index: u32, new_name: String)
```

## Permissions

Required zellij permissions:
- `ReadApplicationState` - To receive TabUpdate events with tab information

## Dependencies

Based on zellij-tab-bar-indexed analysis:

```toml
[dependencies]
zellij-tile = "0.43"

[profile.release]
opt-level = "s"
lto = true
strip = true
```

## Build Target

```toml
# .cargo/config.toml
[build]
target = "wasm32-wasip1"
```

## File Structure

```
zellij-crew/
├── .cargo/
│   └── config.toml       # WASM target configuration
├── src/
│   └── lib.rs            # Plugin implementation
├── Cargo.toml
├── rust-toolchain.toml   # Pin rust version for WASM compatibility
├── DESIGN.md             # This document
└── README.md             # Usage documentation
```

## Edge Cases

1. **Pool exhausted**: New tabs remain unnamed when all pool names are in use
2. **Tab closed**: The assigned name becomes available again for fill-in mode
3. **Tab renamed by user**: If `rename_custom=false`, plugin respects user's choice
4. **Multiple tabs with same name**: Possible in round-robin after cycling; fill-in prevents this
5. **Plugin loaded with existing tabs**: All existing tabs are processed on first TabUpdate

## Future Considerations

Not in scope for initial implementation, but worth noting:

- Multiple themed name pools (switch between NATO, Star Wars, etc.)
- Persist name assignments across zellij restarts
- Tab groups with separate name pools

---

# Phase 2: Tab-Bar Plugin with Activity Indicators

## Motivation

The initial `rename_tab()` approach works but has limitations:

1. **Fragile**: Tab indices shift when tabs close, causing position mismatches
2. **Limited indicators**: Can only append text to names (no colors, no rich formatting)
3. **No state awareness**: Can't show if a terminal is waiting for input vs running

To support inter-agent communication and activity monitoring, we need richer capabilities. The solution is evolving zellij-crew into a full **tab-bar replacement plugin**.

## Research: Existing Zellij Status/Tab Plugins

### zjstatus (https://github.com/dj95/zjstatus)

A highly configurable status bar plugin with:
- Notification widget for alerts
- Command widget for running shell commands at intervals
- Pipe system for external control (`zellij pipe --name zjstatus::notify::message`)

zjstatus is mature and extensible but doesn't do terminal content analysis - it relies on external signals via pipes.

### zj-status-bar (https://github.com/cristiand391/zj-status-bar)

A compact bar plugin with a **tab alert** feature:
- Uses a shell wrapper function `zw()` that executes commands and pipes exit codes
- Shows green/red indicators on inactive tabs when commands complete
- Alerts clear when you focus the tab

Detection method: Explicit shell integration, not automatic.

### claude-code-zellij-status (https://github.com/thoo/claude-code-zellij-status)

**Most relevant to our goals.** Monitors Claude Code activity across Zellij panes:

- Uses Claude Code's hook event system (PreToolUse, PostToolUse, Stop, Notification)
- Claude emits JSON via stdin; a shell script parses events and maps to visual states
- Sends updates to zjstatus via `zellij pipe`
- State persisted in `/tmp/claude-zellij-status/{session}.json`

Visual indicators include:
- Yellow `●` for active work
- Gray `○` for idle
- Blue symbols for reading/searching
- Orange `⚡` for bash execution
- Red `?` for user input required

**Key insight**: Claude Code already emits rich state information via hooks. We can consume these signals directly rather than trying to detect state from terminal output.

### Gap Analysis

| Feature | zjstatus | zj-status-bar | claude-code-zellij-status | zellij-crew (proposed) |
|---------|----------|---------------|---------------------------|------------------------|
| Tab naming | No | No | No | Yes |
| Activity indicators | Via pipe only | Exit codes only | Claude hooks only | Multi-source |
| Terminal content analysis | No | No | No | Yes (fallback) |
| Shell prompt detection | No | No | No | Yes |
| Inter-agent messaging | No | No | No | Planned |

## Architecture

### Core Principle: Separation of Concerns

```
┌─────────────────────────────────────────────────────────────────┐
│                        Signal Sources                           │
├─────────────────────────────────────────────────────────────────┤
│  Pipe Messages    │  Content Analysis  │  Activity Timeout      │
│  (high confidence)│  (medium confidence)│  (uncertainty flag)    │
│                   │                     │                        │
│  - Claude hooks   │  - Prompt regex     │  - No output for N sec │
│  - Shell precmd   │  - Last line check  │  - Mark as "sleeping"  │
│  - Other agents   │  - Question detect  │                        │
└────────┬──────────┴─────────┬───────────┴────────────┬──────────┘
         │                    │                        │
         ▼                    ▼                        ▼
┌─────────────────────────────────────────────────────────────────┐
│                         TabState                                │
│  ─────────────────────────────────────────────────────────────  │
│  Single source of truth for per-pane/tab state                  │
│                                                                 │
│  - Current state (idle, working, waiting, sleeping, etc.)       │
│  - Signal source (pipe, regex, timeout)                         │
│  - Last activity timestamp                                      │
│  - Confidence level                                             │
└────────────────────────────────┬────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                         Renderer                                │
│  ─────────────────────────────────────────────────────────────  │
│  Reads TabState, renders tab bar with indicators                │
│                                                                 │
│  │ alice ○ │ bob ● │ carol ? │ dave 😴 │                        │
│     idle    working  question  sleeping                         │
└─────────────────────────────────────────────────────────────────┘
```

### TabState: The Truth Store

TabState is the central data structure that holds the current state of each pane. It doesn't care where signals come from - it just maintains truth.

```rust
struct TabState {
    panes: HashMap<PaneId, PaneState>,
}

struct PaneState {
    status: PaneStatus,
    source: SignalSource,       // How we know this state
    last_activity: Instant,     // When we last saw output
    last_updated: Instant,      // When state last changed
}

enum PaneStatus {
    Unknown,                    // Just opened, no data yet
    Idle,                       // At shell prompt, waiting for input
    Running,                    // Command executing, output flowing
    Working,                    // Agent actively processing (from hook)
    Waiting,                    // Waiting for user input
    Question,                   // Agent asked a question
    Sleeping,                   // No activity for N seconds (uncertainty)
}

enum SignalSource {
    Pipe,                       // Explicit message (highest confidence)
    ContentMatch,               // Regex matched prompt (medium confidence)
    Timeout,                    // Inferred from silence (low confidence)
    None,                       // No signal yet
}
```

### Signal Priority

Explicit signals always override inferred state:

```
effective_state = pipe_signal ?? content_match ?? timeout_inference ?? Unknown
```

When a pipe message arrives, it immediately sets state and clears any inferred state. Content analysis only runs when there's no recent explicit signal. Timeout detection only kicks in when we have no other information.

## Pipe Protocol

Namespace: `zellij-crew:status` (not `crew-bar` - the plugin may evolve beyond just the bar)

### Message Format

```bash
zellij pipe --name zellij-crew:status --args "pane=PANE_ID,state=STATE"
```

### States

| State | Meaning |
|-------|---------|
| `idle` | At prompt, ready for input |
| `running` | Command executing |
| `working` | Agent actively processing |
| `thinking` | Agent thinking (distinct from working) |
| `tool:NAME` | Agent using specific tool (e.g., `tool:Bash`) |
| `waiting` | Waiting for user input |
| `question` | Agent asked a question |
| `done` | Task completed |
| `error` | Something went wrong |

### Example: Claude Code Hook Integration

In `.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": ["zellij pipe --name zellij-crew:status --args \"pane=$ZELLIJ_PANE_ID,state=tool:$TOOL_NAME\""],
    "PostToolUse": ["zellij pipe --name zellij-crew:status --args \"pane=$ZELLIJ_PANE_ID,state=working\""],
    "Stop": ["zellij pipe --name zellij-crew:status --args \"pane=$ZELLIJ_PANE_ID,state=idle\""],
    "Notification": ["zellij pipe --name zellij-crew:status --args \"pane=$ZELLIJ_PANE_ID,state=question\""]
  }
}
```

### Example: Shell Integration (Optional)

For non-Claude terminals, users can optionally add shell hooks:

```bash
# In .zshrc or .bashrc
precmd() {
    [[ -n "$ZELLIJ_PANE_ID" ]] && zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=idle"
}
preexec() {
    [[ -n "$ZELLIJ_PANE_ID" ]] && zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=running"
}
```

## Content Analysis (Fallback Detection)

When no explicit signals are available, analyze terminal content to infer state.

### Performance Considerations

Terminal output can be large (thousands of lines). Naive regex on every line is expensive.

**Mitigations:**

1. **Debounce**: Only analyze after output stops (100-200ms delay)
2. **Limit scope**: Only check last N characters (default: 500)
3. **Anchor patterns**: Require `^` or `$` to fail fast on non-matches
4. **Skip when explicit**: Don't analyze if we have a recent pipe signal
5. **Rate limit**: Max one analysis per pane per second

### Prompt Detection

```rust
// Configurable patterns, checked against last non-empty line
let prompt_patterns = [
    r"^.*[$#%>❯λ»›]\s*$",      // Common shell prompts
    r"^.*>>>\s*$",              // Python REPL
    r"^irb.*>\s*$",             // Ruby REPL
    r"^.*\?\s*$",               // Ends with question mark
];

fn detect_prompt(last_line: &str, patterns: &[Regex]) -> Option<PaneStatus> {
    let stripped = strip_ansi_codes(last_line);
    for pattern in patterns {
        if pattern.is_match(&stripped) {
            return Some(PaneStatus::Idle);
        }
    }
    None
}
```

### Question Detection

If output ends with `?` or contains patterns like "Would you like", "Do you want", etc., infer `Question` state.

## Timeout Detection (Sleeping State)

When we don't know the state and there's no output for N seconds, show a "sleeping" indicator.

```rust
const DEFAULT_SLEEP_TIMEOUT_SECS: u64 = 30;

fn check_sleeping(pane: &PaneState, timeout: Duration) -> bool {
    pane.status == PaneStatus::Unknown
        && pane.last_activity.elapsed() > timeout
}
```

The sleeping state (`😴`) is honest about uncertainty - we're not claiming the terminal is idle, just that nothing's happening. This catches:

- Programs that don't emit prompts
- Hung processes
- Terminals the user forgot about

## Visual Indicators

| State | Default Indicator | Color |
|-------|-------------------|-------|
| `idle` | `○` | dim/gray |
| `running` | `◐` | blue |
| `working` | `●` | yellow |
| `thinking` | `◑` | yellow |
| `tool:*` | `⚡` | orange |
| `waiting` | `▶` | green |
| `question` | `?` | red |
| `sleeping` | `😴` | dim/gray |
| `done` | `✓` | green |
| `error` | `!` | red |

Indicators are configurable via KDL.

## Configuration

```kdl
plugins {
    crew location="file:zellij_crew.wasm" {
        // Naming (existing)
        names "alice bob carol dave eve frank"
        mode "fill-in"
        show_position "false"

        // Activity detection
        sleep_timeout_secs "30"           // 0 to disable sleeping detection
        analysis_delay_ms "200"           // Debounce before content analysis
        analysis_max_chars "500"          // Only check last N chars

        // Prompt patterns (space-separated, anchored recommended)
        prompt_patterns "[$#%>❯]\\s*$ >>>\\s*$"

        // Indicators (customizable)
        indicator_idle "○"
        indicator_running "◐"
        indicator_working "●"
        indicator_question "?"
        indicator_sleeping "😴"
        indicator_done "✓"
    }
}
```

## Implementation Phases

### Phase 2a: Tab-Bar Foundation
- Fork default tab-bar plugin structure
- Integrate existing naming logic
- Basic rendering with names

### Phase 2b: Pipe Integration
- Subscribe to pipe messages
- Implement TabState
- Update indicators from pipe signals

### Phase 2c: Content Analysis
- Add PaneContents permission
- Implement debounced analysis
- Prompt regex matching

### Phase 2d: Timeout Detection
- Timer-based sleeping detection
- Activity timestamp tracking

### Phase 2e: Polish
- Configurable indicators
- Color theming
- Performance tuning

## Default Tab-Bar Plugin Analysis

Analysis of zellij's default tab-bar plugin (from `default-plugins/tab-bar/src/`).

### File Structure (~620 lines total)

| File | Lines | Purpose |
|------|-------|---------|
| `main.rs` | 164 | Plugin lifecycle, state management, mouse handling |
| `tab.rs` | 141 | Individual tab rendering, multi-user indicators |
| `line.rs` | 465 | Tab line layout, overflow handling, keybind display |

### Key Data Structures

**LinePart** - Core rendering unit that pairs styled content with width:
```rust
pub struct LinePart {
    part: String,              // ANSI-styled text content
    len: usize,                // Visual width (for layout calculations)
    tab_index: Option<usize>,  // For mouse click detection
}
```

**State** - Plugin state:
```rust
struct State {
    tabs: Vec<TabInfo>,
    active_tab_idx: usize,
    mode_info: ModeInfo,           // Contains palette/colors/capabilities
    tab_line: Vec<LinePart>,       // Cached for mouse click handling
    hide_swap_layout_indication: bool,
}
```

### Plugin Lifecycle

**load():**
- Parse config from `BTreeMap<String, String>`
- Call `set_selectable(false)` - tab bar should not steal focus
- Subscribe to: `TabUpdate`, `ModeUpdate`, `Mouse`

**update():**
- `ModeUpdate`: Store `mode_info` (contains colors/palette)
- `TabUpdate`: Store tabs, find `active_tab_idx`
- `Mouse`: Handle `LeftClick` (tab switch), `ScrollUp/Down` (tab navigation)
- Returns `bool` indicating whether to re-render

**render():**
- Build `LinePart` for each tab via `tab_style()`
- Call `tab_line()` to handle layout with overflow
- Print with ANSI background fill to end of line

### Multi-User Indicator Pattern

Located in `tab.rs:7-21, 59-82`. This is the template for our activity indicators:

```rust
fn cursors(focused_clients: &[ClientId], colors: MultiplayerColors) -> (Vec<ANSIString>, usize) {
    let mut cursors = vec![];
    for client_id in focused_clients {
        if let Some(color) = client_id_to_colors(*client_id, colors) {
            // Each user gets a colored space block
            cursors.push(style!(color.1, color.0).paint(" "));
        }
    }
    (cursors, len)
}
```

Rendered after tab name as: `tabname[■ ■]` where each `■` is a colored space.

### Tab Styling Flow

1. `tab_style()` - Entry point, appends "(FULLSCREEN)" or "(SYNC)" if needed
2. `render_tab()` - Builds the actual styled output with separators
3. Uses `palette.ribbon_selected` / `ribbon_unselected` for colors
4. Separator: `""` (powerline arrow) when `capabilities.arrow_fonts`, else empty

### Overflow Handling

When tabs don't fit in available width (`line.rs:15-108`):
- Active tab is always shown
- Adds tabs left/right alternately while they fit
- Shows collapsed indicators: `← +N` (left) and `+N →` (right)
- Clicking collapsed indicator jumps to first hidden tab in that direction

### Dependencies

```toml
ansi_term = "0.12"           # ANSI styling
unicode-width = "0.1.8"      # Unicode character width calculation
zellij-tile                  # Plugin API
zellij-tile-utils            # style!() macro for ANSI styling
```

### Patterns to Adopt

1. **LinePart pattern** - Separate styled content from width tracking
2. **Two-level rendering** - `tab_style()` → `render_tab()` abstraction
3. **Mouse handling** - Cache `tab_line` for click coordinate detection
4. **Palette integration** - Use `mode_info.style.colors` for theming
5. **set_selectable(false)** - Tab bar shouldn't steal keyboard focus
6. **Overflow handling** - Show `← +N` / `+N →` when tabs don't fit

### Activity Indicator Integration Point

In `render_tab()` (tab.rs:59-82), after the tab text but before the right separator,
we can add our activity indicator using the same pattern as multi-user cursors:

```rust
// After existing multi-user cursor section
if let Some(indicator) = get_activity_indicator(tab_id, &tab_state) {
    let indicator_styled = style!(indicator_color, background_color)
        .paint(indicator_symbol);
    s.push_str(&indicator_styled.to_string());
    tab_text_len += indicator_symbol.width();
}
```

## Future: Inter-Agent Messaging

With the pipe infrastructure in place, agents can send messages to each other:

```bash
# Agent in "alice" tab asking "bob" a question
zellij pipe --name zellij-crew:message --args "from=alice,to=bob,msg=What's the API schema?"
```

The plugin could:
1. Route messages between named tabs
2. Show message indicators
3. Provide a message queue that agents can poll

This builds on the foundation of named tabs + activity state to enable true multi-agent coordination.
