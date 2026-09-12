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

The naming daemon is done and tested. The crew CLI is the next piece; `cli/` still
holds the previous tool until then, which is why `make install` currently ships only
the plugin.

## Component 1: Naming daemon (background plugin)

### Loading

Loaded headless via `config.kdl`, exactly one entry:

```kdl
load_plugins {
    "file:~/.config/zellij/zellij-crew.wasm" {
        names "Alice Bob Carol Dave Emma Frank Grace Henry Ivy Jack Kate Luke Mia Nick Olivia Paul Quinn Ryan Sarah Tom Uma Victor Wendy Xavier Yara Zack"
    }
}
```

`names` is one space-separated string. KDL's multi-argument form (`names "Alice"
"Bob"`) is not an error but only the first argument reaches the plugin, so the pool
would be one name. The daemon logs the pool size at load so that is visible in
`zellij.log`. Omit `names`, or leave it empty, to get the built-in default pool.

zellij loads the plugin once at session start with `start_suppressed = true` into
the first tab. It has no viewport and never renders, but its `update()` still
receives every subscribed event: delivery is gated on subscription plus permission,
rendering separately on having a viewport. Every attached client gets its own
instance of the plugin. With identical configuration they compute identical
assignments, so this is invisible apart from doubled log lines. Two `load_plugins`
entries for the same file that differ in spelling or configuration are two daemons
with two permission entries; keep it to one.

### Naming logic

- `load()` requests `ReadApplicationState` and `ChangeApplicationState` together,
  then subscribes to `TabUpdate` and `PermissionRequestResult`. `TabUpdate` is gated
  on the first permission and renaming on the second. The `PermissionRequestResult`
  subscription is load-bearing: that event reaches the plugin only through zellij's
  cached-event replay, which drops unsubscribed event types.
- Every `TabUpdate` is processed; there is no separate "granted" flag. A delivered
  `TabUpdate` already proves `ReadApplicationState`, and both permissions are set
  together, so gating on the result event would only add a way to get stuck.
- Each `TabUpdate` carries a full `Vec<TabInfo>` (`tab_id`, `name`, `position`), but
  `name` is what the tab bar would display, not always the tab's name. With the
  default frame style (`pane_frame_style "titles"`) a tab that still has its default
  name and exactly one tiled pane with an explicit title (a `rename-pane`, or the
  shell's OSC title) is reported under that pane title. An exit-status suffix such
  as ` [ EXITED ] ` is appended even to named tabs. So the daemon resolves any
  reported name that is neither default-looking nor a pool name by id through
  `get_tab_info`, which returns the raw `tab.name`. That call is only made for those
  tabs, so hand-named tabs cost one host call per snapshot and pool-named tabs none.
  `pane_frames false` disables the masking entirely.
- Names in use are the case-folded set of all tabs' real names, matching the CLI's
  case-insensitive `tell` resolution: a hand-named `bob` blocks `Bob`.
- Every tab whose real name matches `Tab #<digits>` gets, in position order, the
  first pool name not in use, via `rename_tab_with_id`. Recomputed from scratch on
  every snapshot, so a stale snapshot re-issues the same rename rather than a new one.
- When the pool is dry the remaining tabs keep their default names. That is logged
  once, on the transition, not per snapshot.

### Why the name is the only "new tab" signal

zellij reuses tab ids. A new tab's id is `highest live id + 1`, so closing the newest
tab frees its id for the next one. "Newness" is therefore never inferred from the
tab id. A tab needs naming if and only if its real name still matches `Tab #N`.
Hand-named tabs and layout-named tabs are left alone. The corollary: `Tab #N` is
reserved. Renaming a tab back to that form by hand hands it to the daemon again.

The default name itself is `Tab #<id + 1>`, so after any close-and-reopen churn an
overflow tab reads `Tab #<something>`, not `Tab #27`.

### Event caching and startup order

zellij caches every event for a plugin from the moment it starts loading until the
post-load replay. On a permission-cache miss, `request_permission` keeps that cache
open until the user answers; on a hit it is answered immediately. Either way the
cached events, including the result and the tab snapshots, are replayed to `update()`
in arrival order, filtered by the plugin's subscriptions at replay time. The order of
`request_permission` and `subscribe` inside `load()` does not matter.

At session start the daemon's hidden pane and its permission request can both arrive
at the screen before any tab exists. Both are parked in the screen's
`pending_events_waiting_for_client` and re-sent, in order, when a tab layout is
applied, a client attaches, or a layout override completes. The daemon sees tabs on
the next tab-state report after its subscription lands; inserting its own pane
usually triggers one, which is why the first tab is normally named at once.

### Lifecycle

- **Detach / reattach**: the server outlives the client. A reattaching client that
  gets the same client id back reuses the running instance with no reload. If some
  other connection holds that id at that moment (an agent running `zellij-crew tell`
  in the detached session, say), the reattach gets a new id and a fresh instance is
  loaded through the permission cache; the old one lingers harmlessly.
- **Resurrection**: session serialization is on by default (60 s interval). If the
  server dies (WSL2 shutting down an idle VM, for instance), reattaching resurrects
  the session and reloads the plugin once. Named tabs come back named and are left
  alone. A tab that was still `Tab #N` at serialization comes back as `Tab #N` and is
  named on resurrection if a pool name is free.

### Permissions

- The first grant surfaces the suppressed plugin as a floating pane with the yes/no
  prompt, inside the tab that owns its hidden pane (the first tab of the session),
  without switching tabs. Answering re-suppresses it.
- **Answer it with `y` or `n`.** While the prompt is floating the daemon is an
  ordinary pane: closing the pane, or closing that tab, unloads the plugin for the
  rest of the session and writes no cache entry. If the server is serialized while
  the prompt is up, the pane is serialized too, and resurrection recreates it as a
  visible blank floating pane running a second instance; close that pane.
- A denial writes an empty entry to the cache, so it is not sticky: the prompt
  returns next session. The only durable opt-out is removing the `load_plugins`
  entry. After `n` the daemon unsubscribes from `TabUpdate`, so apart from a
  one-time burst of denial lines in `zellij.log` for the events cached during the
  prompt, it goes quiet.
- The grant is cached to `~/.cache/zellij/permissions.kdl`, keyed by the plugin's
  absolute path exactly as zellij resolved it from the `load_plugins` entry: tilde
  expanded, no `file:` prefix. Prompted once, and the grant survives resurrection.
- **Pre-seed it.** `make install-permissions` appends the entry for the installed
  path, idempotently, so no session on that host ever shows the prompt. This is also
  how the grant travels to other hosts: commit the resulting `permissions.kdl` or run
  the target there. Format:

  ```kdl
  "/home/karl/.config/zellij/zellij-crew.wasm" {
      ReadApplicationState
      ChangeApplicationState
  }
  ```

### Concurrency with manual renames

A rename computed from a snapshot is applied later by the screen thread, and nothing
re-checks the name at apply time. A script that does `new-tab` followed by
`rename-tab build` can lose the race and end up with a pool name; use `new-tab
--name build` instead, which the daemon never touches. Interactively the window is
milliseconds.

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

Two consumers, two places:

- The **daemon's** name pool lives in `config.kdl`, as `names` under the
  `load_plugins` entry (see Loading above). One knob, no file I/O in the plugin. The
  sandbox preopens `/host`, `/data`, `/cache` and `/tmp`, and `/host` is the entry's
  `cwd`, so the daemon could be pointed at the config dir and read a file there; that
  would add a second setting and a read-at-load path for no gain.
- The **CLI's** settings live in `zellij-crew.kdl`, found next to `config.kdl` using
  the same config-dir resolution zellij uses (`$ZELLIJ_CONFIG_DIR`, else the
  platform default), overridable with `--config` or `$ZELLIJ_CREW_CONFIG`.

```kdl
// tell message wrapping. Placeholders: {id} {from} {to} {message}
tell {
    prefix "[CREW MESSAGE #{id} from {from}; to: {to}] "
    postfix "*CRITICAL* Reply ONLY by running this bash command, do not just output your response: zellij-crew tell {from} \"your reply here\""
    enter_delay_ms 250
}
```

Both work with their config absent, falling back to built-in defaults.

## State and logs

Under zellij's own tmp dir, `/tmp/zellij-<uid>/`, per session:

```
/tmp/zellij-<uid>/zellij-crew/<session>/
    lock             flock guarding the message counter
    next_msg_id      monotonic tell counter
    messages.jsonl   tell log: {id, ts, from, to, pane, msg}
/tmp/zellij-<uid>/zellij-log/zellij-crew.log   CLI diagnostics, next to zellij.log
```

The daemon keeps no state on disk and writes no log file of its own: its few
`eprintln!` lines (pool size at load, pool exhausted, permission denied) land in
`zellij.log`, tagged with the plugin path. Only the CLI's `tell` counter and log
persist, and only those need the lock.

## Build

- One workspace, two crates: `plugin/` (package `zellij-crew-plugin`, target
  `wasm32-wasip1`, installed as `zellij-crew.wasm` so it cannot collide with the CLI
  binary's name) and `cli/` (package `zellij-crew-cli`, targets `x86_64-` and
  `aarch64-unknown-linux-musl`, static).
- `plugin/` depends on `zellij-tile` by path to the sibling `../zellij` checkout;
  `cli/` on `zellij-client` and `zellij-utils` the same way, with the `vendored_curl`
  feature so the musl build is fully static.
- The toolchain is pinned to 1.95.0 to match zellij's `rust-version`. The pinned
  zellij commit is recorded in [UPSTREAM.md](UPSTREAM.md).
- Makefile: `install` (plugin only until the CLI lands), `install-permissions`,
  `reload` for the dev loop, `cross` for aarch64. `reload` hot-reloads only an
  instance with an identical configuration, so pass `NAMES="..."` equal to the
  `config.kdl` value; a mismatch makes zellij start a second, visible instance.

## Edge cases

- **Pool exhausted**: the tab keeps its default `Tab #N` name; the CLI uses whatever
  name the tab has.
- **Several tabs opened at once** (a layout): one snapshot lists them all and the
  single daemon names them in position order, so no lock is needed.
- **A default tab whose single pane's title happens to equal a pool name** is
  indistinguishable from a named tab without a host call per tab, so it is treated as
  named. Accepted; it needs a terminal title of exactly `Alice`.
- **`tell` to an unknown name, or a tab with no terminal pane**: the CLI prints an
  error and exits non-zero.

## Out of scope (possible later)

- **Status indicators**: shelved. The `status` subcommand writes state now so the
  door stays open, but there is no renderer.
- Round-robin naming, broadcast `tell`, and targeting a specific pane id: all easy to
  add later on top of this structure.
