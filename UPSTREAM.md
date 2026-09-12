# Upstream zellij

zellij-crew builds against a sibling checkout of zellij at `../zellij` via path
dependencies, not crates.io. Both crates are pinned to one zellij commit so builds
are reproducible.

## Pinned version

| | |
|--|--|
| Repository | https://github.com/zellij-org/zellij |
| Commit | `a16232338` |
| Workspace version | 0.46.0 |

## Dependencies

| crate | used by | why |
|-------|---------|-----|
| `zellij-tile` | `plugin/` | plugin API: `ZellijPlugin`, `subscribe`, `request_permission`, `rename_tab_with_id` |
| `zellij-client` | `cli/` | connect to the session and send actions over zellij's IPC |
| `zellij-utils` | `cli/` | shared types, session resolution, the `vendored_curl` feature for static musl |

The client/server IPC uses a versioned contract (`CLIENT_SERVER_CONTRACT_VERSION`).
As long as our pinned commit and the installed zellij share that contract version,
the CLI talks to the running session cleanly.

## Bumping

1. Update `../zellij` to the new commit or tag.
2. `cargo build` the workspace; fix any API drift (the plugin API and the `Action`
   enum are the usual movers).
3. Run the [TESTING.md](TESTING.md) suite against a throwaway session.
4. Update the commit and version in the table above.

Keep the installed zellij and this pinned commit on the same contract version.
