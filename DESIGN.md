# zellij-crew Design Document

## Purpose

zellij-crew gives each zellij tab a short, memorable name (Alice, Bob, Carol, ...)
and lets whoever is running in one tab address another by that name. The motivating
case is several Claude Code agents in one zellij session that need to talk to each
other: "Alice, take the frontend; Bob, take the API."

Two independent jobs:

1. **Automatic tab naming** — every new tab gets the first free name from a pool.
2. **Inter-tab messaging** — `zellij-crew tell <name> <message>` delivers a message
   into another tab.

## Architecture

zellij-crew is deliberately small. Two components, each driven by zellij's own
mechanisms rather than reimplementing them:

| Component | Form | Job |
|-----------|------|-----|
| Naming daemon | headless background wasm plugin | rename new `Tab #N` tabs from the pool |
| crew CLI | native musl binary linking `zellij-client` / `zellij-utils` | `tell` / `list` / `name` / `status` |

## Component 1: Naming daemon (background plugin)

### Loading

Loaded headless via `config.kdl`:

```kdl
load_plugins {
    "file:~/.config/zellij/zellij-crew.wasm"
}
```

zellij loads it once at session start with `start_suppressed = true`. It has no
viewport and never renders, but its `update()` still receives every subscribed
event (event delivery is gated on subscription plus permission; rendering is a
separate path gated on having a viewport).

### Naming logic

- On `load`, subscribe to `TabUpdate`.
- Request permissions on the **first** `TabUpdate`, not in `load()`. Requesting too
  early can hit zellij's "plugin not yet attached to a tab" path, which defers the
  request. By the first `TabUpdate` a tab and client demonstrably exist.
- Each `TabUpdate` carries a full `Vec<TabInfo>` (`tab_id`, `name`, `position`).
- For every tab whose name still matches the default `Tab #<n>`, assign the first
  pool name not currently used by any tab and call `rename_tab_with_id(tab_id, name)`
  (rename by stable id, so it is focus-independent).
- Names in use are read live from the snapshot, so fill-in is automatic: close Bob's
  tab and the next new tab reuses "Bob".

### Why the name is the only "new tab" signal

zellij reuses tab ids. A new tab's id is `highest live id + 1`, so closing the
newest tab frees its id for the next one. "Newness" is therefore never inferred from
the tab id. A tab needs naming if and only if its live name still matches `Tab #N`.
Hand-named tabs and layout-named tabs are left alone.

### Lifecycle

- **Detach / reattach**: the server outlives the client and does not reload plugins
  on attach, so the daemon keeps running with no restart.
- **Resurrection**: session serialization is on by default (60s interval). If the
  server dies (for example WSL2 shutting down an idle VM), reattaching resurrects the
  session and reloads the plugin. This is harmless: resurrected tabs come back with
  their real names, which no longer match `Tab #N`, so nothing is renamed. The freshly
  loaded plugin receives cached events and sees the current tabs immediately.

### Permissions

- Needs `ReadApplicationState` (to receive `TabUpdate`) and `ChangeApplicationState`
  (to rename tabs). Request both together.
- First grant: zellij surfaces the suppressed plugin as a floating pane showing the
  yes/no prompt, then re-suppresses it. This handling has existed since zellij 0.41.
- The grant is cached to `~/.cache/zellij/permissions.kdl`, keyed by plugin path.
  Prompted once, and the grant survives resurrection.
- **Pre-seed for infrastructure-as-code**: grant once on one host, commit the
  generated `permissions.kdl`, distribute it, and no other host prompts. Format:

  ```kdl
  "file:/home/karl/.config/zellij/zellij-crew.wasm" {
      ReadApplicationState
      ChangeApplicationState
  }
  ```

## Component 2: crew CLI

A native binary, `zellij-crew`, that links `zellij-client` and `zellij-utils` and
talks to the running session over zellij's own IPC. No subprocess spawning, no
protocol reimplementation; the library owns the transport.

### Commands

| Command | Job |
|---------|-----|
| `tell <name> <message...>` | deliver a message into the named tab's pane |
| `list [--json]` | tabs with id, name, and last message to / from |
| `name` | print this pane's tab name (for scripts and prompts) |
| `status <state>` | write this tab's status to the state dir; no zellij call, reserved for a future status-indicator revival |

### How `tell` delivers

1. Resolve the destination tab by name (case-insensitive) from the live tab and pane
   list.
2. Pick the destination pane: prefer the pane whose running command is `claude`,
   otherwise the tab's active pane.
3. Write the formatted message with `WriteCharsToPaneId`, then after `enter_delay_ms`
   write a carriage return with `WriteToPaneId`, so Enter arrives as a separate pty
   read. Sent in one write, some pty setups fold it into the message and the line is
   never submitted.

The message wrapping is configurable (see below). Delivery is keystrokes into a pty:
if the target pane is at a permission prompt or in an editor, the text goes there.
That is inherent and documented, not prevented.

## Configuration

`zellij-crew.kdl`, found next to zellij's `config.kdl` using the same config-dir
resolution zellij uses (`$ZELLIJ_CONFIG_DIR`, else the platform default), overridable
with `--config` or `$ZELLIJ_CREW_CONFIG`.

```kdl
// Pool of tab names, first unused wins.
names "Alice Bob Carol Dave Emma Frank Grace Henry Ivy Jack Kate Luke Mia Nick Olivia Paul Quinn Ryan Sarah Tom Uma Victor Wendy Xavier Yara Zack"

// tell message wrapping. Placeholders: {id} {from} {to} {message}
tell {
    prefix "[CREW MESSAGE #{id} from {from}; to: {to}] "
    postfix "*CRITICAL* Reply ONLY by running this bash command, do not just output your response: zellij-crew tell {from} \"your reply here\""
    enter_delay_ms 250
}
```

The daemon reads `names`; the CLI reads `tell`. Both work with the file absent,
falling back to built-in defaults.

## State and logs

Under zellij's own tmp dir, `/tmp/zellij-<uid>/`, per session:

```
/tmp/zellij-<uid>/zellij-crew/<session>/
    lock             flock guarding the message counter
    next_msg_id      monotonic tell counter
    messages.jsonl   tell log: {id, ts, from, to, pane, msg}
/tmp/zellij-<uid>/zellij-log/zellij-crew.log   diagnostics, next to zellij.log
```

Naming keeps no state on disk. The daemon holds it in memory and re-derives from
`TabUpdate` after any reload. Only the CLI's `tell` counter and log persist, and only
those need the lock.

## Build

- One workspace, two crates: `plugin/` (target `wasm32-wasip1`) and `cli/`
  (targets `x86_64-` and `aarch64-unknown-linux-musl`, static).
- `plugin/` depends on `zellij-tile` by path to the sibling `../zellij` checkout.
- `cli/` depends on `zellij-client` and `zellij-utils` by path, with the
  `vendored_curl` feature so the musl build is fully static.
- The pinned zellij commit is recorded in [UPSTREAM.md](UPSTREAM.md).
- Makefile targets: `build`, `install` (wasm to `~/.config/zellij`, cli to
  `~/.local/bin`), and a cross target for aarch64.

## Edge cases

- **Pool exhausted** (27th tab): the tab stays `Tab #27`; the CLI uses whatever name
  the tab has.
- **Several tabs opened at once** (a layout): each `TabUpdate` is processed by the one
  daemon in order, so no lock is needed for naming.
- **`tell` to an unknown name, or a tab with no terminal pane**: the CLI prints an
  error and exits non-zero.

## Out of scope (possible later)

- **Status indicators**: shelved. The `status` subcommand writes state
  now so the door stays open, but there is no renderer.
- Round-robin naming, broadcast `tell`, and targeting a specific pane id: all easy to
  add later on top of this structure.
