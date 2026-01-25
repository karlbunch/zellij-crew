# zellij-crew

A zellij plugin that automatically renames tabs using names from a configurable pool.

## Installation

Build the plugin:

```bash
cargo build --release
```

The WASM binary will be at `target/wasm32-wasip1/release/zellij_crew.wasm`.

Copy it to your zellij plugins directory or reference it directly in your configuration.

## Architecture

Crew uses a **leader/renderer architecture**:

- **Leader** (loaded via `load_plugins`): Manages tab names, handles renames, tracks state
- **Renderers** (loaded per-tab in layout): Display tab bar with names from leader

This architecture allows multiple tab-bar instances while maintaining a single source of truth for tab names.

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
    crew location="file:/path/to/zellij_crew.wasm" {
        names "alice bob carol dave eve frank grace henry iris jack"
        mode "fill-in"
        show_position "false"
        rename_custom "false"
    }
}

load_plugins {
    "crew"
}
```

The plugin loads in the background - no pane needed. Restart zellij for config changes to take effect.

### Options

| Option | Values | Default | Description |
|--------|--------|---------|-------------|
| `names` | space-separated | NATO phonetic | Pool of names to assign |
| `mode` | `round-robin`, `fill-in` | `round-robin` | Allocation strategy |
| `show_position` | `true`, `false` | `false` | Show position: "alpha <1>" |
| `rename_custom` | `true`, `false` | `false` | Rename user-named tabs |

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

## Permissions

The plugin requires `ReadApplicationState` permission to receive tab updates. Zellij will prompt for permission on first load.

## License

MIT
