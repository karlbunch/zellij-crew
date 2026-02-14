# zellij-crew

A zellij tab-bar plugin that automatically names tabs and displays activity status indicators.

**Features:**
- **Auto-naming**: Tabs get memorable names from a configurable pool (NATO phonetic, Star Wars, custom)
- **Activity indicators**: Visual status per tab (🫥 🥱 🤖 🙋 😴 👀 🔔)
- **External control**: Update status via pipe messages (integrates with Claude Code, shell hooks)
- **Smart allocation**: Fill-in or round-robin modes
- **Self-organizing**: All instances are tab-bar panes; they elect a leader among themselves

## Installation

Build and install:

```bash
make install    # builds WASM and copies to ~/.config/zellij/
```

Or manually:

```bash
cargo build --target wasm32-wasip1 --release
cp target/wasm32-wasip1/release/zellij-crew.wasm ~/.config/zellij/
```

### Makefile Targets

| Target | Description |
|--------|-------------|
| `make build` | Build the WASM plugin |
| `make install` | Build and copy to `~/.config/zellij/` |
| `make reload` | Install and hot-reload in current session |
| `make clean` | `cargo clean` |

## Architecture

Crew uses a **pure tab-bar architecture with leader election**:

- All instances are tab-bar panes (no `load_plugins` needed)
- On startup, instances elect a leader using a simple protocol (highest plugin_id wins)
- The **leader** manages tab names, handles renames, tracks activity status, AND renders
- **Renderers** display the tab bar using state broadcast from the leader
- When the leader's tab closes, survivors inherit state and elect a new leader
- `start-or-reload-plugin` triggers clean leadership handoff via BeforeClose/resign

See [DESIGN.md](DESIGN.md) for architecture details and [PROTOCOL.md](PROTOCOL.md) for message specs.

## Usage

**1. Define the plugin in config.kdl:**

```kdl
plugins {
    crew location="file:~/.config/zellij/zellij-crew.wasm" {
        names "Alice Bob Carol Dave Emma Frank Grace Henry Ivy Jack"
        mode "fill-in"
    }
}
```

**2. Use in layout:**

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

Save as `~/.config/zellij/layouts/crew-bar.kdl` and start with:
```bash
zellij --layout crew-bar
```

No `load_plugins` block is needed. The tab-bar instances self-organize via leader election.

## Configuration

Add to your zellij config (`~/.config/zellij/config.kdl`):

```kdl
plugins {
    crew location="file:~/.config/zellij/zellij-crew.wasm" {
        names "alice bob carol dave eve frank grace henry iris jack"
        mode "fill-in"
        hide_swap_layout_indication "false"
    }
}
```

The plugin runs as a tab-bar pane in each tab (via layout). Instances elect a leader among themselves. Restart zellij for config changes to take effect.

### Options

| Option | Values | Default | Description |
|--------|--------|---------|-------------|
| `names` | space-separated | NATO phonetic | Pool of names to assign |
| `mode` | `round-robin`, `fill-in` | `round-robin` | Allocation strategy |
| `hide_swap_layout_indication` | `true`, `false` | `false` | Hide swap layout status in tab bar |

**Note:** `show_position` feature (showing "alpha <1>" style names) is planned but not yet implemented.

### Allocation Modes

**round-robin**: Names assigned sequentially. When a tab is closed, the next new tab gets the next name in sequence (doesn't reuse the freed name).

**fill-in**: Names assigned to fill gaps. When a tab is closed, its name becomes available for the next new tab.

## Example Name Pools

NATO phonetic (default):
```
alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey xray yankee zulu
```

Star Wars:
```
luke leia han chewie vader yoda obiwan palpatine r2d2 c3po boba lando mace windu anakin padme
```

Greek letters:
```
alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega
```

## Activity Status System

Crew displays activity indicators next to tab names to show what's happening in each tab.

### Status Indicators

| Status | Emoji | Meaning | Trigger |
|--------|-------|---------|---------|
| Unknown | 🫥 | No status information yet | New tab |
| Idle | 🥱 | Ready for input, at prompt | External signal |
| Working | 🤖 | Agent actively processing | External signal |
| Question | 🙋 | Agent has a question | External signal |
| Sleeping | 😴 | No activity (unused) | (Not yet implemented) |
| Watching | 👀 | Monitoring/observing | External signal |
| Attention | 🔔 | Needs attention | External signal |

### Updating Status via Pipe

External tools update status by sending pipe messages:

```bash
# Update by pane ID (requires ZELLIJ_PANE_ID environment variable)
zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=working"

# Update by tab name
zellij pipe --name zellij-crew:status --args "name=alice,state=attention"
```

**Valid states:** `unknown`, `idle`, `working`, `question`, `sleeping`, `watching`, `attention`

### Claude Code Integration

Add to `.claude/settings.json` in your project:

```json
{
  "hooks": {
    "PreToolUse": ["zellij-crew-claude working"],
    "Stop": ["zellij-crew-claude idle"],
    "Notification": ["zellij-crew-claude attention"]
  }
}
```

The `zellij-crew-claude` script is located in `bin/zellij-crew-claude` in this repo.

### Shell Integration (Optional)

For non-Claude terminals, add to `.zshrc` or `.bashrc`:

```bash
# Only run in zellij
if [[ -n "$ZELLIJ" ]] && [[ -n "$ZELLIJ_PANE_ID" ]]; then
    precmd() {
        zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=idle" 2>/dev/null
    }
    preexec() {
        zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=working" 2>/dev/null
    }
fi
```

## Querying Status

Crew provides commands to query current tab status.

### Help

```bash
zellij pipe --name zellij-crew:status --args help
```

Shows all available states and usage examples.

### List Tabs

```bash
# Human-readable format
zellij pipe --name zellij-crew:status --args list

# JSON format
zellij pipe --name zellij-crew:status --args "format=json,list"
```

**Example output:**
```
ID    Name    Status
--    ----    ------
1     alice   🤖 working
2     bob     🥱 idle
3     carol   🔔 attention
```

### Detailed State (Agent Coordination)

```bash
# Via CLI
zellij-crew state

# Via pipe
zellij pipe --name zellij-crew:status --args "format=json,state_query"
```

Returns JSON with per-tab pane metadata, message tracking timestamps, and status age -- designed for boss agents coordinating workers. See [PROTOCOL.md](PROTOCOL.md) for the full schema.

## Permissions

The plugin requires these permissions:
- `ReadApplicationState` - To receive tab updates and pane information
- `ChangeApplicationState` - To rename tabs
- `MessageAndLaunchOtherPlugins` - To broadcast state between plugin instances
- `ReadCliPipes` - To receive status updates from external tools

Zellij will prompt for permissions on first load.

## License

MIT
