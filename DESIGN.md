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
