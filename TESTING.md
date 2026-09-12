# Testing

All tests run in a throwaway zellij session with its own config, so your real session
and config are never involved. The cases below are what the naming daemon has been
verified against; the CLI cases describe the planned tool.

## Throwaway session harness

Give the session its own config pointing at the built wasm:

```kdl
// zztest-config.kdl
default_shell "/bin/bash"
load_plugins {
    "file:/abs/path/to/target/wasm32-wasip1/release/zellij-crew-plugin.wasm" {
        names "Alice Bob Carol Dave Emma"   // small pool makes exhaustion cheap
    }
}
```

Start it from a non-tty shell by giving it a pty with `script(1)`, then drive it with
`zellij -s zztest action ...`:

```bash
setsid script -qfc "zellij -s zztest --config zztest-config.kdl" /dev/null \
    </dev/null >/tmp/zztest.log 2>&1 &

zellij -s zztest action query-tab-names          # raw tab names
zellij -s zztest action list-panes -j | jq '.[] | select(.is_plugin)'

zellij kill-session zztest && zellij delete-session zztest
```

Harness notes:

- `query-tab-names` and `list-tabs` return the raw server-side names. The pane-title
  masking the daemon has to see through (N9) only appears in the plugin's `TabUpdate`,
  so it is verified by outcome, not by inspection.
- **Resurrection needs the config too:** `zellij --config zztest-config.kdl attach
  zztest`, otherwise the resurrected server runs your real config.
- **Killing the test client:** anchor the pattern, `pkill -f '^zellij (-s
  zztest|attach zztest)'`. An unanchored `pkill -f` matches the shell running the test
  script and kills it.
- **Did the plugin reload?** Count `Loaded plugin '<wasm path>'` lines in
  `/tmp/zellij-<uid>/zellij-log/zellij.log`. Instance count:
  `list-panes -j | jq '[.[] | select(.is_plugin)] | length'`.
- The daemon's `eprintln!` lines land in that same `zellij.log` at DEBUG level.
- zellij opens its own "About Zellij" tips popup as a floating plugin pane in the
  first tab of a fresh session. It is not ours; ignore it in pane listings.

## Permissions

Start each of these with no entry for the wasm in `~/.cache/zellij/permissions.kdl`
(remove the file if it only contains test entries).

| # | Case | Expected |
|---|------|----------|
| P1 | Fresh session, then `action write-chars y` | `dump-screen` shows the prompt listing both permissions before; after: prompt gone, the plugin pane is `suppressed=true floating=false`, `permissions.kdl` gains an entry keyed by the bare absolute wasm path, and `query-tab-names` shows `Alice` (cached snapshot replayed) |
| P1-deny | Fresh session, then `action write-chars n`, then open two tabs | tabs stay `Tab #N`; one `zellij-crew: permission denied` line; a single burst of host `Event 'TabUpdate' denied` lines at the moment of denial and none afterwards; `permissions.kdl` gains an **empty** entry for the path, which means the prompt returns next session |
| P1-close | Fresh session, close the prompt pane or its tab instead of answering | plugin unloaded for the session, no cache entry, no naming; the recovery is a new session |
| P3 | Pre-seeded entry (`make install-permissions`, or by hand) | no prompt, `Alice` immediately |

## Naming daemon

| # | Case | Expected |
|---|------|----------|
| N1 | Fresh session, open two tabs | `Alice`, `Bob`, `Carol` |
| N2 | Split a pane inside a named tab | no rename |
| N3 | Close `Bob`, open a new tab | new tab named `Bob` (fill-in) |
| N4 | Open several tabs at once | all distinct names |
| N5 | Hand-rename a tab, open a new tab | hand-named tab untouched; new tab gets next free name |
| N5b | Hand-rename a tab to `dave`, open new tabs | `Dave` is skipped (case-folded), `Emma` comes next |
| N6 | Exhaust the pool | overflow tabs keep their default `Tab #N` names; exactly one `pool exhausted` line in `zellij.log` however many snapshots follow |
| N7 | Kill the attached client, reattach with nothing else connected | names unchanged, one instance, no new `Loaded plugin` line. With a CLI connection held open across the reattach, expect one new load and naming still working |
| N8 | `kill-session`, then `attach` with the config | `EXITED - attach to resurrect` beforehand; afterwards names kept, one instance, one new `Loaded plugin` line, no tab renamed |
| N8b | Kill and resurrect while the first-run prompt is still floating | a visible blank floating plugin pane in the first tab and two `Loaded plugin` lines; close that pane |
| N9 | On an overflow (default-named) tab run `action rename-pane shelltitle`, then close named tabs to free names | the daemon sees `shelltitle`, resolves the raw name by id, and the tab gets a pool name; without that resolution it would stay `Tab #N` forever |
| N10 | Hand-rename a named tab to `Tab #9` with the pool full, then close a named tab | it stays `Tab #9` until a name frees up, then is named: `Tab #N` is reserved and the daemon reacts to closes, not only creations |

Scripts that create and immediately rename a tab should use `new-tab --name`; a
`new-tab` followed by `rename-tab` can lose the race to the daemon.

## crew CLI (planned)

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
