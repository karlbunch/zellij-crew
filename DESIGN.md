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

        // Per-status indicator overrides (all optional, defaults to emoji)
        // Set to "" to suppress the [brackets] entirely for that state
        status_unknown ""       // hide indicator when no status
        // status_idle "🥱"     // default
        // status_working "🤖"  // default
        // status_question "🙋" // default
        // status_sleeping "😴" // default
        // status_watching "👀" // default
        // status_attention "🔔" // default
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
| `status_unknown` | string | `"🫥"` | Indicator for unknown state (`""` to hide) |
| `status_idle` | string | `"🥱"` | Indicator for idle state (`""` to hide) |
| `status_working` | string | `"🤖"` | Indicator for working state (`""` to hide) |
| `status_question` | string | `"🙋"` | Indicator for question state (`""` to hide) |
| `status_sleeping` | string | `"😴"` | Indicator for sleeping state (`""` to hide) |
| `status_watching` | string | `"👀"` | Indicator for watching state (`""` to hide) |
| `status_attention` | string | `"🔔"` | Indicator for attention state (`""` to hide) |

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

All instances are tab-bar panes. They elect a leader among themselves.

```rust
struct State {
    // Common state (all instances are tab-bar panes)
    instance_id: String,
    plugin_id: u32,           // From get_plugin_ids(), election tiebreaker (highest wins)
    mode_info: ModeInfo,
    config: Config,
    is_leader: bool,          // Determined by election protocol
    election_pending: bool,   // Waiting for election timeout

    // Leader-only state
    known_tabs: HashMap<u32, CrewTabState>,  // tab_id -> CrewTabState (source of truth)
    pane_manifest: Option<PaneManifest>,     // For mapping pane_id -> tab_id
    last_assigned_idx: Option<usize>,        // For round-robin mode
    inherited_state: Option<HashMap<u32, CrewTabState>>,  // From leader resign

    // All instances (for rendering)
    received_tabs: Vec<CrewTabState>,  // From leader broadcast (renderers only)
    tabs: Vec<TabInfo>,                // From TabUpdate
    active_tab_idx: usize,
    tab_line: Vec<LinePart>,           // Cached for mouse clicks
}

struct CrewTabState {
    tab_id: u32,                     // Stable ID from "Tab #N" (key in HashMap)
    name: String,                    // Current name ("Alice" after rename, "Tab #5" before)
    pending_rename: Option<String>,  // Some("Alice") when rename sent, waiting for confirmation
    user_defined: bool,              // true if user named it, false if from our pool
    status: ActivityStatus,          // Current activity status
}

struct Config {
    names: Vec<String>,           // Name pool
    mode: AllocationMode,         // round-robin or fill-in
    hide_swap_layout_indication: bool,  // Whether to hide swap layout status
    status_indicators: HashMap<ActivityStatus, String>,  // Per-status display overrides
    // show_position feature planned but not implemented
}

enum AllocationMode {
    RoundRobin,
    FillIn,
}

enum ActivityStatus {
    Unknown,    // Just opened, no data yet
    Idle,       // At shell prompt, waiting for input
    Working,    // Agent actively processing
    Question,   // Agent asked a question
    Sleeping,   // No activity (timeout)
    Watching,   // Monitoring/observing
    Attention,  // Needs user attention
}
```

**Key Design Decisions:**

- **tab_id as HashMap key**: Tab IDs are stable (never change, even when tabs close/reorder). Parsed from "Tab #N" default name. Required for `rename_tab(tab_id, name)` API.
- **pending_rename flag**: Prevents infinite rename loops. When we call `rename_tab()`, the next TabUpdate may still show the old name. We track pending renames to avoid re-renaming the same tab.
- **Leader election**: All instances are tab-bar panes. They elect a leader using a ping/ack/claim protocol with highest-plugin_id-wins tiebreaker. No `load_plugins` needed.
- **Broadcast not query**: Leader broadcasts state to all renderers on every change. This is more efficient than renderers querying leader, and mirrors how zellij handles multi-user focus indicators.

## Event Flow

### Plugin Lifecycle

1. **load()**: Parse configuration from `BTreeMap<String, String>`, initialize state
2. **update()**: Handle events (primarily `TabUpdate`)
3. **render()**: Not used for tab renaming (we use actions, not UI)

### Tab Rename Flow (Leader Only)

Only the leader instance handles renaming. Renderers never call rename_tab().

```
TabUpdate event received (LEADER ONLY)
    │
    ├─► Store tabs for pane_id → tab_id mapping
    │
    ├─► For each tab in update:
    │       │
    │       ├─► Is default name "Tab #N"?
    │       │       │
    │       │       YES → Parse tab_id from name
    │       │             │
    │       │             ├─► Already in known_tabs?
    │       │             │       YES → Check pending_rename
    │       │             │             │
    │       │             │             └─► If pending, wait for confirmation
    │       │             │       NO  → New tab, allocate from pool
    │       │             │             rename_tab(tab_id, new_name)
    │       │             │             Add to known_tabs with pending_rename: Some(new_name)
    │       │       │
    │       │       NO → Non-default name (renamed or user-defined)
    │       │             │
    │       │             └─► Check pending_rename in known_tabs
    │       │                     │
    │       │                     └─► If matches: rename confirmed!
    │       │                         Clear pending_rename, update name
    │       │
    │       └─► Mark tab_id as still present
    │
    └─► For tabs no longer present (closed):
            │
            ├─► Remove from known_tabs
            └─► If user_defined: false → name returns to pool (fill-in mode)

    Finally: broadcast_state() → all renderers receive updated CrewTabState
```

**Rename Confirmation Loop:**

1. Leader calls `rename_tab(5, "Alice")`, stores `pending_rename: Some("Alice")`
2. Next TabUpdate may still show "Tab #5" (rename not processed yet)
3. Leader sees tab 5 with pending_rename → skips (no re-rename)
4. Later TabUpdate shows "Alice" → matches pending_rename
5. Leader confirms: `name = "Alice"`, `pending_rename = None`
6. No infinite loop

### Parsing Default Tab Names

Default names follow pattern `Tab #N` where N is the **tab ID needed for rename_tab()**.
This is a workaround for the plugin API not exposing tab IDs directly.

```rust
fn parse_default_name(name: &str) -> Option<u32> {
    // "Tab #5" → Some(5) (the tab_id for rename_tab)
    if name.starts_with("Tab #") {
        name[5..].parse().ok()
    } else {
        None
    }
}
```

### Avoiding Rename Loops

The leader tracks all known tab names. When TabUpdate arrives after a rename:
1. Tab now has name "alice" (we renamed it)
2. Leader checks: is "alice" in known_tabs? YES → skip
3. No rename loop

User-renamed tabs are also tracked (with `user_defined: true`) so we don't try to rename them.

## Actions

The plugin uses zellij's action system to rename tabs:

```rust
// Rename tab using ID parsed from "Tab #N"
rename_tab(tab_id: u32, new_name: String)
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

### Pure Tab-Bar Architecture with Leader Election

All instances are tab-bar panes. No `load_plugins` required. Instances self-organize
using an election protocol.

```
    ┌───────────────────┬───────────────────┬───────────────────┐
    ▼                   ▼                   ▼                   ▼
[crew-bar 1]       [crew-bar 2]       [crew-bar 3]       [crew-bar N]
 LEADER             renderer           renderer           renderer
 plugin_id=2        plugin_id=4        plugin_id=6        plugin_id=2N
 Manages state      Receives state     Receives state     Receives state
 AND renders        Renders            Renders            Renders
```

#### Election Protocol

On startup, each instance broadcasts a `crew-leader-ping` and sets a 0.3s timeout:

1. **Ping**: Broadcast `crew-leader-ping` with own plugin_id
2. **Ack**: If a leader exists, it responds with `crew-leader-ack` + serialized state
3. **Claim**: If no ack within 0.3s, broadcast `crew-leader-claim` and become leader
4. **Tiebreaker**: If two claims race, highest plugin_id wins (newer instances get higher IDs)
5. **Resign**: On `BeforeClose`, leader broadcasts `crew-leader-resign` + state; survivors re-elect

See [PROTOCOL.md](PROTOCOL.md) for message format details.

#### Why Highest Plugin ID Wins

Newer instances get higher plugin IDs in zellij. This means:
- `start-or-reload-plugin` replacements naturally win elections
- Fresh code always takes over from stale code
- No ambiguity about which instance is "newer"

#### Configuration

**config.kdl** - Define the plugin:
```kdl
plugins {
    crew location="file:~/.config/zellij/zellij-crew.wasm" {
        names "Alice Bob Carol ..."
        mode "fill-in"
    }
}
```

**layouts/crew-bar.kdl** - Use in tab template:
```kdl
layout {
    default_tab_template {
        pane size=1 borderless=true {
            plugin location="crew"
        }
        children
        pane size=1 borderless=true {
            plugin location="status-bar"
        }
    }
}
```

#### Tab Name Stability

After `rename_tab()` succeeds:
- `tab.name` changes from "Tab #1" to "alpha"
- Name is now STABLE across position changes
- When middle tab closes, remaining tabs keep their names
- Track assigned names by `tab.name` (not position)

This solves the original position-based bug where names would shift when middle tabs closed.

#### Edge Cases

| Scenario | Behavior |
|----------|----------|
| First startup (1 tab) | 0.3s delay, then leader claims, renames tab |
| New tab opens | New instance pings, leader acks, renderer stays |
| `start-or-reload-plugin` | BeforeClose resign, new instance wins election |
| Leader tab closed | BeforeClose resign, survivors elect with inherited state |
| Leader crash (no BeforeClose) | Tab names survive (already renamed). New tab triggers election. Status resets to Unknown. |
| Simultaneous tab opens | Race resolved by highest plugin_id |

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

### CrewTabState: The Truth Store (Leader Only)

The leader maintains CrewTabState for all tabs. This is the single source of truth that gets
broadcast to renderers. Renderers are stateless - they just paint what the leader tells them.

```rust
// Leader's state
struct State {
    known_tabs: HashMap<u32, CrewTabState>,  // Keyed by stable tab_id
    last_assigned_idx: Option<usize>,        // For round-robin mode
    config: Config,                          // Name pool, mode, etc.
}

struct CrewTabState {
    tab_id: u32,                     // Stable ID from "Tab #N" (explicit, no iteration needed)
    name: String,                    // Current name ("Alice" after rename, "Tab #5" before)
    pending_rename: Option<String>,  // Some("Alice") when rename sent, waiting for confirmation
    user_defined: bool,              // true = user named it, false = from our pool
    status: ActivityStatus,          // Current activity state
}

enum ActivityStatus {
    Unknown,                    // Just opened, no data yet
    Idle,                       // At shell prompt, waiting for input
    Working,                    // Agent actively processing (from hook)
    Question,                   // Agent asked a question
    // Future: Running, Waiting, Sleeping, etc.
}
```

**Why tab_id as HashMap key:**

- Tab IDs are stable (never change, even when tabs close/reorder)
- Parsed from "Tab #N" default name
- Needed for `rename_tab(tab_id, name)` API anyway
- Storing in struct too (redundant with key) for self-documentation

**Why pending_rename flag:**

Prevents infinite rename loops:
1. Leader calls `rename_tab(5, "Alice")`, sets `pending_rename: Some("Alice")`
2. Next TabUpdate still has "Tab #5" (rename not confirmed yet)
3. Leader sees tab_id=5 in known_tabs with pending_rename → skip
4. Later TabUpdate has "Alice" → match against pending_rename, confirm, clear flag
5. No re-renaming, no loops

### Name Pool Management

When a tab closes:
- If `user_defined: false` → name returns to pool (available for fill-in mode)
- If `user_defined: true` → name is discarded (was user's choice, don't reuse)

```rust
fn on_tab_closed(&mut self, name: &str) {
    if let Some(tab) = self.tabs.remove(name) {
        if !tab.user_defined && self.name_pool.contains(&tab.name) {
            // Pool name available again for fill-in
        }
        // User-defined names just disappear
    }
    self.broadcast_state();
}
```

### Renderer State

Renderers maintain no state of their own. They receive CrewTabState from the leader and render:

```rust
// Renderer receives broadcast from leader
fn render_tabs(&self, tabs: &[CrewTabState], config: &Config) {
    for tab in tabs {
        let indicator = match tab.status {
            ActivityStatus::Idle => &config.indicator_idle,
            ActivityStatus::Working => &config.indicator_working,
            ActivityStatus::Question => &config.indicator_question,
            // etc.
        };
        // Output: "Alice [●]"
        print_tab(&tab.name, indicator);
    }
}
```

### Leader-to-Renderer Pipe Communication

The leader broadcasts state to all renderer instances using zellij's pipe messaging system.

**Implementation Requirements:**

1. **Permission**: Request `PermissionType::MessageAndLaunchOtherPlugins` in load()
2. **ZellijPlugin trait**: Implement `fn pipe(&mut self, PipeMessage) -> bool` method
3. **Broadcasting**: Use `pipe_message_to_plugin()` with `with_plugin_url("crew")`

**Leader broadcast code:**

```rust
fn broadcast_state(&self) {
    let tabs: Vec<&CrewTabState> = self.known_tabs.values().collect();

    if let Ok(json) = serde_json::to_string(&tabs) {
        pipe_message_to_plugin(
            MessageToPlugin::new("crew-state")
                .with_plugin_url("crew")  // Routes to ALL instances with URL "crew"
                .with_payload(json)
        );
    }
}
```

**Renderer receive code:**

```rust
fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
    if !self.is_leader && pipe_message.name == "crew-state" {
        if let Some(payload) = pipe_message.payload {
            if let Ok(tabs) = serde_json::from_str::<Vec<CrewTabState>>(&payload) {
                self.received_tabs = tabs;
                return true;  // Request render
            }
        }
    }
    false
}
```

**Key Insights:**

- `pipe()` method (not `update()` with CustomMessage events)
- `with_plugin_url("crew")` without `with_destination_plugin_id()` = broadcast to all instances
- PipeMessage.source is `Plugin(leader_id)` when sent from plugin
- All crew instances (leader + renderers) receive the message, filter by `is_leader` flag
- No registration needed - zellij routes by plugin URL automatically

### Signal Priority

Explicit signals always override inferred state:

```
effective_state = pipe_signal ?? content_match ?? timeout_inference ?? Unknown
```

When a pipe message arrives, it immediately sets state and clears any inferred state. Content analysis only runs when there's no recent explicit signal. Timeout detection only kicks in when we have no other information.

## Pipe Protocols

crew uses two separate pipe namespaces:

1. **crew-state** (internal): Leader → Renderers state broadcast
2. **zellij-crew:status** (external): External sources → Leader activity updates

### Internal Pipe: crew-state

Used by the leader to broadcast CrewTabState to all renderer instances. See "Leader-to-Renderer Pipe Communication" section above.

- **Name**: `crew-state`
- **Payload**: JSON array of CrewTabState objects
- **Direction**: Leader → All Renderers
- **Trigger**: Every TabUpdate that changes state

### External Pipe: zellij-crew:status

Used by external tools (Claude Code, shell hooks, etc.) to update activity status.

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

## Implementation Status

**✅ Completed:**
- Tab-bar foundation (forked from zellij default tab-bar)
- Pure tab-bar architecture with leader election (no `load_plugins` needed)
- Leader election protocol (ping/ack/claim/resign with highest-plugin_id tiebreaker)
- Name allocation (round-robin and fill-in modes)
- Activity status system (7 states with emoji indicators)
- Pipe integration (external control via zellij-crew:status)
- State broadcasting (leader → renderers via crew-state pipe)
- State inheritance on leader handoff (BeforeClose resign)
- Pane ID → tab mapping (via PaneManifest)
- Name-based and pane-based status updates
- Built-in help and list commands
- Hook script (bin/zellij-crew-claude)
- Configurable indicators (custom emoji/text per state via `status_*` config keys)
- Makefile with build/install/reload/clean targets

**📋 Planned (see Future Enhancements below):**
- Content analysis (automatic state detection from terminal output)
- Timeout detection (sleeping state when no activity)
- show_position feature (display "alpha <1>" style names)
- Inter-agent messaging (tab-to-tab communication)

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

---

# Future Enhancements

This section documents planned features that are not yet implemented.

## show_position Feature

Display tab position in names: "alpha <1>", "bravo <2>", etc.

**Status:** Config field removed (was dead code). Needs implementation.

**Design:** Append position to name in renderer, not in CrewTabState (keeps leader state clean).

## Content Analysis (Automatic State Detection)

Analyze terminal output to infer activity state when no explicit signals available.

**Challenges:**
- Performance (terminal output can be large)
- False positives (matching "$ " in program output, not prompts)
- Privacy (reading terminal content)

**Approach:**
- Debounced analysis (200ms after output stops)
- Last N characters only (default: 500)
- Regex patterns for prompt detection
- Skip when recent pipe signal exists

## Timeout Detection (Sleeping State)

Mark tabs as "sleeping" (😴) when no activity for N seconds.

**Design:** Timer-based, tracks last activity timestamp per tab. Honest about uncertainty - we're not claiming idle, just that nothing's happening.

## Inter-Agent Messaging

Enable tab-to-tab communication using named tabs as addresses.

**Protocol:**
```bash
zellij pipe --name zellij-crew:message --args "from=alice,to=bob,msg=What's the API schema?"
```

**Implementation:**
1. Leader receives message, stores in per-tab queue
2. Target tab polls for messages (via pipe query)
3. Leader shows message indicator (📬) on tab bar

**Use case:** AI agents in different tabs can coordinate work, share context, ask each other questions.

## Configurable Indicators

**Status: ✅ Implemented**

Each activity state's display indicator can be overridden via `status_*` plugin config keys.
Setting a key to `""` suppresses the `[brackets]` entirely for that state. Omitting a key
uses the default emoji.

**Config keys:** `status_unknown`, `status_idle`, `status_working`, `status_question`, `status_sleeping`, `status_watching`, `status_attention`

**Config example:**
```kdl
crew location="..." {
    names "alice bob carol"
    status_unknown ""          // hide indicator when unknown (no brackets shown)
    status_working "WRK"       // custom text shown as [WRK]
    status_idle "💤"           // custom emoji
    status_question "❓"
    // status_sleeping omitted → uses default 😴
}
```

**Implementation:** `Config::indicator_for()` in `src/main.rs` returns `Some(&str)` for the display string or `None` to suppress brackets. `ActivityStatus::default_indicator()` provides the fallback emoji for each state.
