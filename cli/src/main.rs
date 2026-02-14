use serde_json::Value;
use std::env;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process;

const VALID_STATES: &[&str] = &[
    "unknown", "idle", "working", "question", "sleeping", "watching", "attention",
];

struct HookMapping {
    event: &'static str,
    state: &'static str,
    matcher: Option<&'static str>,
}

const HOOK_MAPPINGS: &[HookMapping] = &[
    HookMapping { event: "SessionStart",       state: "watching",  matcher: None },
    HookMapping { event: "UserPromptSubmit",    state: "working",   matcher: None },
    HookMapping { event: "PreToolUse",          state: "working",   matcher: Some("*") },
    HookMapping { event: "PostToolUse",         state: "working",   matcher: Some("*") },
    HookMapping { event: "SubagentStart",       state: "working",   matcher: Some("*") },
    HookMapping { event: "SubagentStop",        state: "working",   matcher: Some("*") },
    HookMapping { event: "Stop",                state: "idle",      matcher: None },
    HookMapping { event: "Notification",        state: "attention", matcher: Some("*") },
    HookMapping { event: "PermissionRequest",   state: "question",  matcher: Some("*") },
    HookMapping { event: "SessionEnd",          state: "unknown",   matcher: None },
];

fn cli_path() -> String {
    "\"$HOME/.config/zellij/zellij-crew\"".to_string()
}

fn print_help() {
    eprintln!("zellij-crew - CLI companion for zellij-crew plugin");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  zellij-crew status <state>          Send status update to plugin");
    eprintln!("  zellij-crew tell <name> <message>   Send message to another tab");
    eprintln!("  zellij-crew --setup                 Install hooks into ~/.claude/settings.json");
    eprintln!("  zellij-crew --remove                Remove hooks from ~/.claude/settings.json");
    eprintln!("  zellij-crew --help                  Show this help");
    eprintln!();
    eprintln!("Valid states:");
    for s in VALID_STATES {
        eprintln!("  {}", s);
    }
    eprintln!();
    eprintln!("Hook mappings (installed by --setup):");
    for h in HOOK_MAPPINGS {
        let m = h.matcher.unwrap_or("-");
        eprintln!("  {:25} -> {:10} (matcher: {})", h.event, h.state, m);
    }
}

// ============================================================================
// Settings management (--setup / --remove)
// ============================================================================

fn settings_path() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| {
        eprintln!("zellij-crew: $HOME not set");
        process::exit(1);
    });
    PathBuf::from(home).join(".claude").join("settings.json")
}

fn read_settings(path: &PathBuf) -> Value {
    if !path.exists() {
        return serde_json::json!({"hooks": {}});
    }
    let data = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("zellij-crew: failed to read {}: {}", path.display(), e);
        process::exit(1);
    });
    serde_json::from_str(&data).unwrap_or_else(|e| {
        eprintln!("zellij-crew: failed to parse {}: {}", path.display(), e);
        process::exit(1);
    })
}

fn write_settings(path: &PathBuf, value: &Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("zellij-crew: failed to create {}: {}", parent.display(), e);
            process::exit(1);
        });
    }
    let json = serde_json::to_string_pretty(value).unwrap_or_else(|e| {
        eprintln!("zellij-crew: failed to serialize settings: {}", e);
        process::exit(1);
    });
    fs::write(path, json + "\n").unwrap_or_else(|e| {
        eprintln!("zellij-crew: failed to write {}: {}", path.display(), e);
        process::exit(1);
    });
}

fn has_our_hook(entry: &Value) -> bool {
    if let Some(hooks) = entry.get("hooks").and_then(|h| h.as_array()) {
        for hook in hooks {
            if let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) {
                // Matches both old "zellij-crew-claude" and new "zellij-crew" hooks
                if cmd.contains("zellij/zellij-crew") {
                    return true;
                }
            }
        }
    }
    false
}

fn make_hook_entry(mapping: &HookMapping) -> Value {
    let command = format!("{} status {}", cli_path(), mapping.state);
    let mut entry = serde_json::Map::new();
    if let Some(m) = mapping.matcher {
        entry.insert("matcher".to_string(), Value::String(m.to_string()));
    }
    entry.insert(
        "hooks".to_string(),
        serde_json::json!([{"type": "command", "command": command}]),
    );
    Value::Object(entry)
}

fn do_setup() {
    let path = settings_path();
    let mut settings = read_settings(&path);
    let mut installed = 0u32;
    let mut skipped = 0u32;

    if settings.get("hooks").is_none() {
        settings
            .as_object_mut()
            .unwrap()
            .insert("hooks".to_string(), serde_json::json!({}));
    }

    let hooks = settings["hooks"].as_object_mut().unwrap_or_else(|| {
        eprintln!("zellij-crew: .hooks is not an object in settings.json");
        process::exit(1);
    });

    for mapping in HOOK_MAPPINGS {
        let event_array = hooks
            .entry(mapping.event)
            .or_insert_with(|| Value::Array(vec![]));

        let arr = event_array.as_array_mut().unwrap_or_else(|| {
            eprintln!(
                "zellij-crew: .hooks.{} is not an array in settings.json",
                mapping.event
            );
            process::exit(1);
        });

        let already = arr.iter().any(|e| has_our_hook(e));
        if already {
            skipped += 1;
        } else {
            arr.push(make_hook_entry(mapping));
            installed += 1;
        }
    }

    write_settings(&path, &settings);
    eprintln!(
        "zellij-crew: installed {} hooks, {} already present ({})",
        installed, skipped, path.display()
    );
}

fn do_remove() {
    let path = settings_path();
    if !path.exists() {
        eprintln!("zellij-crew: {} not found, nothing to remove", path.display());
        return;
    }

    let mut settings = read_settings(&path);
    let mut removed = 0u32;

    if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        let events: Vec<String> = hooks.keys().cloned().collect();
        for event in &events {
            if let Some(arr) = hooks.get_mut(event).and_then(|v| v.as_array_mut()) {
                let before = arr.len();
                arr.retain(|e| !has_our_hook(e));
                removed += (before - arr.len()) as u32;
            }
        }
        let empty_events: Vec<String> = hooks
            .iter()
            .filter(|(_, v)| v.as_array().is_some_and(|a| a.is_empty()))
            .map(|(k, _)| k.clone())
            .collect();
        for event in empty_events {
            hooks.remove(&event);
        }
    }

    write_settings(&path, &settings);
    eprintln!(
        "zellij-crew: removed {} hooks ({})",
        removed, path.display()
    );
}

// ============================================================================
// Subcommands
// ============================================================================

fn require_zellij() -> String {
    if env::var("ZELLIJ").is_err() {
        process::exit(0);
    }
    match env::var("ZELLIJ_PANE_ID") {
        Ok(id) => id,
        Err(_) => process::exit(0),
    }
}

fn do_status(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: zellij-crew status <state>");
        eprintln!("Valid states: {}", VALID_STATES.join(" "));
        process::exit(1);
    }

    let pane_id = require_zellij();
    let state = args[0].as_str();

    if !VALID_STATES.contains(&state) {
        eprintln!("zellij-crew: invalid state '{}'", state);
        eprintln!("Valid states: {}", VALID_STATES.join(" "));
        process::exit(1);
    }

    let pipe_args = format!("pane={},state={}", pane_id, state);
    let err = process::Command::new("zellij")
        .args(["pipe", "--name", "zellij-crew:status", "--args", &pipe_args])
        .exec();
    eprintln!("zellij-crew: failed to exec zellij: {}", err);
    process::exit(1);
}

fn do_tell(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: zellij-crew tell <name> <message...>");
        process::exit(1);
    }

    let pane_id = require_zellij();
    let dest = &args[0];
    let message = args[1..].join(" ");

    let pipe_args = format!("to={},pane={}", dest, pane_id);
    let err = process::Command::new("zellij")
        .args([
            "pipe", "--name", "zellij-crew:msg",
            "--args", &pipe_args,
            "--", &message,
        ])
        .exec();
    eprintln!("zellij-crew: failed to exec zellij: {}", err);
    process::exit(1);
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        print_help();
        process::exit(1);
    }

    match args[0].as_str() {
        "--help" | "-h" => print_help(),
        "--setup" => do_setup(),
        "--remove" => do_remove(),
        "status" => do_status(&args[1..]),
        "tell" => do_tell(&args[1..]),
        other => {
            eprintln!("zellij-crew: unknown command '{}'", other);
            eprintln!("Run with --help for usage");
            process::exit(1);
        }
    }
}
