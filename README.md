# zellij-crew

Give every zellij tab a short, memorable name (Alice, Bob, Carol, ...) and let
whoever runs in one tab message another by name. Built for coordinating several
Claude Code agents in one zellij session.

Two pieces:

- a **headless background plugin** that auto-names new tabs, and
- a **`zellij-crew` CLI** that sends messages between tabs.

Small and headless: the naming plugin has no UI, and there is no status bar or
background polling. See [DESIGN.md](DESIGN.md) for how it works and
[UPSTREAM.md](UPSTREAM.md) for the zellij version it builds against.

## Install

Requires a sibling checkout of zellij at `../zellij` (the crates are path
dependencies). See [UPSTREAM.md](UPSTREAM.md) for the pinned commit.

```bash
make install              # plugin -> ~/.config/zellij/zellij-crew.wasm, CLI -> ~/.local/bin/zellij-crew
make install-permissions  # grants the plugin read/change state ahead of time, no prompt
```

The CLI is a static musl binary, so linking it needs `musl-tools` (Debian/Ubuntu)
or `musl` (Void). `make build-cli-native` builds a non-static one without that.

Then enable the naming daemon in `~/.config/zellij/config.kdl`:

```kdl
load_plugins {
    "file:~/.config/zellij/zellij-crew.wasm" {
        names "Alice Bob Carol Dave Emma Frank Grace Henry Ivy Jack"
    }
}
```

`names` is one space-separated string; omit it for the built-in 26-name pool.

Without the pre-seeded grant, the first session shows a one-time yes/no prompt as a
floating pane in the first tab. Answer it with `y`; closing that pane instead unloads
the daemon for the session. Details in [DESIGN.md](DESIGN.md#permissions).

## Use

New tabs are named automatically. To message another tab:

```bash
zellij-crew tell Bob "can you take the API tests?"
zellij-crew list            # tabs, names, recent messages
zellij-crew list --json
zellij-crew name            # this pane's tab name
zellij-crew config          # effective settings and paths
```

## Configuration

The name pool is set on the `load_plugins` entry in `config.kdl` (`names "..."`).
The CLI's message wrapping lives in an optional `zellij-crew.kdl` next to
`config.kdl`. Defaults are built in for both; the settings are only needed to change
them. See [DESIGN.md](DESIGN.md#configuration) for the schema.

## Build targets

| Target | Description |
|--------|-------------|
| `make install` | build the plugin and the static CLI and copy both into place |
| `make install-permissions` | pre-seed the plugin's grant in `~/.cache/zellij/permissions.kdl` |
| `make reload NAMES="..."` | reinstall and hot-reload the daemon in the running session; `NAMES` must match `config.kdl` |
| `make build-cli-native` | non-static CLI for local testing, no musl toolchain needed |
| `make cross` | cross-build the CLI for aarch64 musl |
| `make clean` | `cargo clean` |

## License

MIT
