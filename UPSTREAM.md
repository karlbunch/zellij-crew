# Upstream Source

This plugin is based on the official zellij tab-bar plugin.

## Source

**Repository:** https://github.com/zellij-org/zellij
**Path:** `default-plugins/tab-bar/src/`
**Commit:** `889bd06f447f9d50ee46b8efc7d9970d3693cb96`
**Date:** 2025-10-21
**Message:** Migrate from wasmtime to wasmi (#4449)

## Architecture

This plugin is a full rewrite of the tab-bar with crew functionality fully integrated.

| File | Status | Notes |
|------|--------|-------|
| `line.rs` | Verbatim copy | No modifications from upstream |
| `tab.rs` | Verbatim copy | No modifications from upstream |
| `main.rs` | Complete rewrite | 817 lines - all crew logic integrated |

**Important:** Unlike typical forks, main.rs is NOT a lightly-modified upstream file. It's a complete reimplementation that combines:
- Tab-bar rendering (from upstream)
- Leader/renderer architecture
- Name allocation and tracking
- Activity status system
- Pipe message handling

**There is no separate crew.rs module.** All logic lives in main.rs.

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

**WARNING:** Do NOT sync main.rs. It's a complete rewrite and cannot be mechanically merged with upstream. If upstream changes tab-bar's API or event handling, manual porting is required.
