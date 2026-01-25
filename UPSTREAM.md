# Upstream Source

This plugin is based on the official zellij tab-bar plugin.

## Source

**Repository:** https://github.com/zellij-org/zellij
**Path:** `default-plugins/tab-bar/src/`
**Commit:** `889bd06f447f9d50ee46b8efc7d9970d3693cb96`
**Date:** 2025-10-21
**Message:** Migrate from wasmtime to wasmi (#4449)

## Architecture

All customizations are isolated in `crew.rs`. Upstream files have minimal hooks.

| File | Status | Notes |
|------|--------|-------|
| `line.rs` | Verbatim copy | No modifications |
| `tab.rs` | Verbatim copy | No modifications |
| `main.rs` | Minimal hooks | ~5 hook points (see below) |
| `crew.rs` | Custom | All zellij-crew logic |

## Hooks in main.rs

```rust
mod crew;                           // 1. Import crew module
crew: crew::CrewState,              // 2. State field
crew::on_load(&mut self.crew, &config);        // 3. In load()
crew::on_update(&mut self.crew, &event);       // 4. In update()
crew::on_tab_update(&mut self.crew, &tabs);    // 5. In TabUpdate handler
crew::get_tab_name(&mut self.crew, t);         // 6. In render() loop
```

Also: `pub` visibility on LinePart fields, `PermissionRequestResult` event subscription.

## Syncing with upstream

The tab-bar source changes infrequently. To check for updates:

```bash
cd /path/to/zellij
git log --oneline -5 -- default-plugins/tab-bar/src/
```

To sync (safe - line.rs and tab.rs are verbatim):
```bash
cp /path/to/zellij/default-plugins/tab-bar/src/line.rs src/
cp /path/to/zellij/default-plugins/tab-bar/src/tab.rs src/
```

For main.rs, diff and manually apply the hook pattern:
```bash
diff -u /path/to/zellij/default-plugins/tab-bar/src/main.rs src/main.rs
```
