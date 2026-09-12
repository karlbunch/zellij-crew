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

## crew CLI

Run the binary from outside the session, acting as a pane by setting the two
variables every real pane has: `ZELLIJ_SESSION_NAME=zztest ZELLIJ_PANE_ID=0
zellij-crew ...` makes it "the first pane of tab 0", so `tell` reports the sender as
that tab's name and `name` prints it.

To see exactly what a receiving pane gets, run a raw-tty reader in it that logs every
`read()` with a timestamp; then the message and the trailing `\r` show up as two
reads with the configured gap between them. A fake `claude` on the server's `PATH`
tests the pane preference, but it must keep `claude` as its argv[0]
(`exec -a claude python3 reader.py ...`): zellij reports the foreground process as
the pane command, so a plain wrapper script would read as `python3`.

| # | Case | Expected |
|---|------|----------|
| C1 | `tell bob "hi"` from pane 0 (case-insensitive name) | `msg#1 sent to Bob on pane N`; the pane sees one read with `\n[CREW MESSAGE #1 from Alice; to: Bob] hi\n<postfix>\n`, then a second read `\r` about 250 ms later |
| C1b | `--config` pointing at a file with a custom `tell` block | prefix, postfix and `enter_delay_ms` from the file are used |
| C2 | Bob's tab has a `claude` pane and focus is on another pane | the `claude` pane receives the message, the focused one does not |
| C3 | `tell nobody hi` | error listing the tabs that exist, exit 1 |
| C4 | no `ZELLIJ_SESSION_NAME` with two sessions running | "several zellij sessions are running", exit 1 |
| C5 | `list` / `list --json` | one row per tab with id, position, name, pane count, status, last message to/from |
| C6 | `name` from pane 0 and pane 1 | `Alice`, `Bob`; without `ZELLIJ_PANE_ID`: "not inside a zellij pane", exit 1 |
| C7 | `status working` then `list` | the status shows on that pane's tab |
| C8 | after three tells | `next_msg_id` is 4, `messages.jsonl` has three lines, `zellij-crew.log` three lines |
| C9 | `config` | prints the config path (and whether it exists), session, state dir, log path, and the effective `tell` settings |
