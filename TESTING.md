# Testing

All tests run in a throwaway zellij session so your real session is never touched.

## Throwaway session harness

Start a disposable session from a non-tty shell by giving it a pty with `script(1)`:

```bash
setsid script -qfc "zellij -s zztest options --default-shell /bin/bash" /dev/null \
    </dev/null >/tmp/zztest.log 2>&1 &

# drive it
zellij -s zztest action new-tab
zellij -s zztest action query-tab-names
zellij -s zztest action list-panes -t

# tear down
zellij kill-session zztest && zellij delete-session zztest
```

To test the naming daemon, point the session's config at a build that has
`load_plugins` set, or load the plugin into the running throwaway session with
`zellij -s zztest action start-or-reload-plugin file:...`.

### zellij CLI notes

- `dump-screen` is written by the server; omit `--path` to get it on the CLI's stdout.
- `zellij action` ignores `ZELLIJ_SESSION_NAME` when only one session is running; use
  `-s <name>`.
- `list-panes -t` joins columns with exactly two spaces.
- `rename-tab`'s help says "focused pane" but it renames the focused tab; the plugin
  uses `rename_tab_with_id` to stay focus-independent.

## Naming daemon

| # | Case | Expected |
|---|------|----------|
| N1 | Fresh session, open two tabs | `Alice`, `Bob`, `Carol` |
| N2 | Split a pane inside a named tab | no rename |
| N3 | Close `Bob`, open a new tab | new tab named `Bob` (fill-in) |
| N4 | Open several tabs at once (layout) | all distinct names |
| N5 | Hand-rename a tab, open a new tab | hand-named tab untouched; new tab gets next free name |
| N6 | Exhaust the pool (27 tabs) | 27th stays `Tab #27` |
| N7 | Detach and reattach (server alive) | names unchanged, daemon not restarted |
| N8 | Kill and resurrect the session | resurrected tabs keep their names; nothing renamed |

For N7/N8, confirm the daemon is still the single running instance and that no tab
was renamed away from its real name.

## Permissions

| # | Case | Expected |
|---|------|----------|
| P1 | First load, empty permission cache | suppressed plugin surfaces as a floating pane with the yes/no prompt; grant re-suppresses it |
| P2 | Second session, cache present | no prompt |
| P3 | Pre-seeded `permissions.kdl` | no prompt on any host |

## crew CLI

| # | Case | Expected |
|---|------|----------|
| C1 | `tell Bob "hi"` from another tab | message text then Enter arrive in Bob's pane as separate reads |
| C2 | `tell` targets the `claude` pane | when a tab has a `claude` pane, that pane receives it |
| C3 | `tell <unknown>` | error, non-zero exit |
| C4 | `tell` to a tab with no terminal pane | error, non-zero exit |
| C5 | `list` / `list --json` | tabs with id, name, last message to/from |
| C6 | `name` | prints this pane's tab name |
| C7 | message id increments | `next_msg_id` advances; `messages.jsonl` gets one line per `tell` |

Verify C1/C2 by dumping the receiving pane:

```bash
zellij -s zztest action dump-screen -p terminal_<id>
```
