//! zellij-crew naming daemon.
//!
//! A headless background plugin (loaded via `load_plugins` in config.kdl) that
//! gives every tab still carrying zellij's default `Tab #N` name the first unused
//! name from a pool. It never renders; it only reacts to `TabUpdate`.
//!
//! Configuration (child of the `load_plugins` entry), a single string value:
//!   names "Alice Bob Carol ..."   space-separated pool; empty or absent = default pool

use std::collections::{BTreeMap, HashSet};
use zellij_tile::prelude::*;

const DEFAULT_NAMES: &str = "Alice Bob Carol Dave Emma Frank Grace Henry Ivy Jack Kate Luke \
                             Mia Nick Olivia Paul Quinn Ryan Sarah Tom Uma Victor Wendy Xavier \
                             Yara Zack";

#[derive(Default)]
struct State {
    names: Vec<String>,
    /// True while the pool is dry, so exhaustion is logged on the transition only.
    exhausted: bool,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // zellij caches every event for a plugin from the moment it starts loading
        // until the post-load replay. On a permission-cache miss, request_permission
        // keeps that cache open until the user answers; the replay then delivers the
        // cached events in order. `TabUpdate` is gated on ReadApplicationState and
        // renaming on ChangeApplicationState, so both are requested together.
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
        // PermissionRequestResult reaches this plugin only through that cached-event
        // replay, which drops unsubscribed event types: keep it subscribed for as
        // long as the Denied arm below exists.
        subscribe(&[EventType::TabUpdate, EventType::PermissionRequestResult]);

        let pool: Vec<String> = configuration
            .get("names")
            .map(String::as_str)
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        self.names = if pool.is_empty() {
            DEFAULT_NAMES.split_whitespace().map(str::to_owned).collect()
        } else {
            pool
        };
        eprintln!("zellij-crew: {} names in pool", self.names.len());
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            // A delivered TabUpdate already proves ReadApplicationState, and both
            // permissions are granted together, so there is no separate gate.
            Event::TabUpdate(tabs) => self.name_tabs(&tabs),
            Event::PermissionRequestResult(PermissionStatus::Denied) => {
                // Otherwise the host keeps routing every TabUpdate here and logs a
                // denial for each one for the rest of the session.
                unsubscribe(&[EventType::TabUpdate]);
                eprintln!("zellij-crew: permission denied, tabs will not be named this session");
            },
            _ => {},
        }
        false // headless: nothing to render
    }
}

impl State {
    /// Rename every tab that still has a default name. Recomputed from scratch on
    /// each snapshot, so it is idempotent on stale snapshots.
    fn name_tabs(&mut self, tabs: &[TabInfo]) {
        // `TabUpdate` reports a default-named single-pane tab under its pane title
        // (a shell's OSC title, for instance) and appends exit-status suffixes, so any
        // reported name that is neither default nor a pool name is resolved by id to
        // the raw tab name.
        let real: Vec<(usize, usize, String)> = tabs
            .iter()
            .map(|t| {
                let name = if is_default_name(&t.name) || self.in_pool(&t.name) {
                    t.name.clone()
                } else {
                    get_tab_info(t.tab_id).map(|raw| raw.name).unwrap_or_else(|| t.name.clone())
                };
                (t.tab_id, t.position, name)
            })
            .collect();

        // Case-folded, matching the CLI's case-insensitive name resolution.
        let mut used: HashSet<String> = real.iter().map(|(_, _, n)| n.to_lowercase()).collect();
        let mut unnamed: Vec<&(usize, usize, String)> =
            real.iter().filter(|(_, _, n)| is_default_name(n)).collect();
        unnamed.sort_by_key(|(_, position, _)| *position);

        let mut exhausted = false;
        for (tab_id, _, _) in unnamed {
            match self.names.iter().find(|n| !used.contains(&n.to_lowercase())) {
                Some(name) => {
                    used.insert(name.to_lowercase());
                    rename_tab_with_id(*tab_id as u64, name);
                },
                None => {
                    exhausted = true;
                    break;
                },
            }
        }
        if exhausted && !self.exhausted {
            eprintln!("zellij-crew: name pool exhausted, leaving default names in place");
        }
        self.exhausted = exhausted;
    }

    fn in_pool(&self, name: &str) -> bool {
        self.names.iter().any(|n| n.eq_ignore_ascii_case(name))
    }
}

/// zellij's default tab name is `Tab #<n>`.
fn is_default_name(name: &str) -> bool {
    match name.strip_prefix("Tab #") {
        Some(rest) => !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}
