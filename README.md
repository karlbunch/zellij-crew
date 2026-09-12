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
make install
```

This builds the wasm plugin to `~/.config/zellij/zellij-crew.wasm` and the CLI to
`~/.local/bin/zellij-crew`.

Then enable the naming daemon in `~/.config/zellij/config.kdl`:

```kdl
load_plugins {
    "file:~/.config/zellij/zellij-crew.wasm"
}
```

On first session start zellij asks once to grant the plugin permission to read and
change application state (it renames tabs). To skip the prompt across many hosts, see
the pre-seed note in [DESIGN.md](DESIGN.md#permissions).

## Use

New tabs are named automatically. To message another tab:

```bash
zellij-crew tell Bob "can you take the API tests?"
zellij-crew list            # tabs, names, recent messages
zellij-crew list --json
zellij-crew name            # this pane's tab name
```

## Configuration

Optional `zellij-crew.kdl` next to your `config.kdl` sets the name pool and the
message wrapping. Defaults are built in; the file is only needed to change them. See
[DESIGN.md](DESIGN.md#configuration) for the schema.

## Build targets

| Target | Description |
|--------|-------------|
| `make build` | build the wasm plugin and the CLI |
| `make install` | build, then copy plugin and CLI into place |
| `make cross` | cross-build the CLI for aarch64 musl |
| `make clean` | `cargo clean` |

## License

MIT
