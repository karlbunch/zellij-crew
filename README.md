# zellij-crew

A zellij tab-bar plugin that automatically names tabs and displays activity status indicators.

**Features:**
- **Auto-naming**: Tabs get memorable names from a configurable pool (NATO phonetic, Star Wars, custom)
- **Activity indicators**: Visual status per tab (🫥 🥱 🤖 🙋 😴 👀 🔔)
- **External control**: Update status via pipe messages (integrates with Claude Code, shell hooks)
- **Smart allocation**: Fill-in or round-robin modes
- **Leader/renderer architecture**: Single binary, multiple instances, one source of truth

## Installation

Build the plugin:

```bash
cargo build --release
```

The WASM binary will be at `target/wasm32-wasip1/release/zellij-crew.wasm`.

Copy it to your zellij plugins directory or reference it directly in your configuration.

## Architecture

Crew uses a **leader/renderer architecture** for efficient state management:

- **Leader** (loaded via `load_plugins`): Manages tab names, handles renames, tracks activity status
- **Renderers** (loaded per-tab in layout): Display tab bar with names and activity indicators from leader

**How it works:**
- Single WASM binary serves both roles (detected by render dimensions on first call)
- Leader tracks state per tab (name, activity status, user-defined flag)
- Leader broadcasts state to all renderers via pipe messages
- Renderers are stateless - they just paint what leader broadcasts
- Activity status updates from external tools (Claude Code, shell hooks) go to leader
- All instances share same binary, no code duplication

See [DESIGN.md](DESIGN.md) for detailed architecture documentation.

## Usage

**1. Configure in config.kdl:**

```kdl
plugins {
    crew location="file:~/.config/zellij/zellij-crew.wasm" {
        names "Alice Bob Carol Dave Emma Frank Grace Henry Ivy Jack"
        mode "fill-in"
    }
}

load_plugins {
    "crew"  # Leader instance (background)
}
```

**2. Use in layout (optional):**

```kdl
layout {
    default_tab_template {
        pane size=1 borderless=true {
            plugin location="crew"  # Renderer per tab
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

## Configuration

Add to your zellij config (`~/.config/zellij/config.kdl`):

```kdl
plugins {
    crew location="file:/path/to/zellij-crew.wasm" {
        names "alice bob carol dave eve frank grace henry iris jack"
        mode "fill-in"
        hide_swap_layout_indication "false"
    }
}

load_plugins {
    "crew"
}
```

The plugin loads in the background via `load_plugins` (leader instance) and in each tab via layout (renderer instances). Restart zellij for config changes to take effect.

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

## Permissions

The plugin requires these permissions:
- `ReadApplicationState` - To receive tab updates and pane information
- `ChangeApplicationState` - To rename tabs
- `MessageAndLaunchOtherPlugins` - To broadcast state between plugin instances
- `ReadCliPipes` - To receive status updates from external tools

Zellij will prompt for permissions on first load.

## License

MIT
