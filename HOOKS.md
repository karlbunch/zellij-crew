# Hook Integration Guide

This document explains how to integrate external tools with zellij-crew's activity status system.

## Overview

External tools can update tab activity status by sending pipe messages to the crew leader instance. This enables rich status indicators for:
- AI coding assistants (Claude Code, Aider, etc.)
- Shell commands (via precmd/preexec hooks)
- Build systems
- Long-running processes
- Custom monitoring tools

## Quick Start

The simplest way to integrate is using the provided hook script:

```bash
# In your zellij pane
zellij-crew-claude working    # Show 🤖 working indicator
zellij-crew-claude idle        # Show 🥱 idle indicator
zellij-crew-claude attention   # Show 🔔 attention indicator
```

The script is located at `bin/zellij-crew-claude` in this repository.

## Available States

| State | Indicator | Use Case |
|-------|-----------|----------|
| `unknown` | 🫥 | Reset to unknown state |
| `idle` | 🥱 | At prompt, ready for input |
| `working` | 🤖 | Agent actively processing |
| `question` | 🙋 | Agent has a question |
| `sleeping` | 😴 | Inactive/paused |
| `watching` | 👀 | Monitoring/observing |
| `attention` | 🔔 | Needs user attention |

## Claude Code Integration

Claude Code provides hook events that can trigger status updates.

### Step 1: Copy Hook Script

```bash
# Make hook script available in PATH
cp bin/zellij-crew-claude ~/.local/bin/
chmod +x ~/.local/bin/zellij-crew-claude
```

### Step 2: Configure Claude Code Hooks

Add to your project's `.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": ["zellij-crew-claude working"],
    "PostToolUse": ["zellij-crew-claude working"],
    "Stop": ["zellij-crew-claude idle"],
    "Notification": ["zellij-crew-claude attention"]
  }
}
```

Or add to global settings at `~/.claude/settings.json` to apply to all projects.

### Hook Events

Claude Code provides these hook events:

- **PreToolUse**: Before Claude executes a tool (Read, Write, Bash, etc.)
- **PostToolUse**: After tool execution completes
- **Stop**: When Claude stops working (waits for user input)
- **Notification**: When Claude sends a notification (questions, errors)

### Advanced: Hook Variables

Claude Code provides environment variables to hooks:

```bash
# Use TOOL_NAME to show different states for different tools
if [ "$TOOL_NAME" = "Bash" ]; then
    zellij-crew-claude watching
else
    zellij-crew-claude working
fi
```

**Available variables:**
- `$TOOL_NAME` - Name of tool being executed
- `$ZELLIJ_PANE_ID` - Current pane ID (auto-set by zellij)

## Shell Integration

For non-Claude terminals, integrate with your shell's precmd/preexec hooks.

### Bash

Add to `~/.bashrc`:

```bash
# Only run in zellij
if [[ -n "$ZELLIJ" ]] && [[ -n "$ZELLIJ_PANE_ID" ]]; then
    # Show idle when prompt appears
    PROMPT_COMMAND='zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=idle" 2>/dev/null'

    # Show working when command starts
    preexec() {
        zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=working" 2>/dev/null
    }
    trap 'preexec' DEBUG
fi
```

### Zsh

Add to `~/.zshrc`:

```bash
# Only run in zellij
if [[ -n "$ZELLIJ" ]] && [[ -n "$ZELLIJ_PANE_ID" ]]; then
    # Show idle when prompt appears
    precmd() {
        zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=idle" 2>/dev/null
    }

    # Show working when command starts
    preexec() {
        zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=working" 2>/dev/null
    }
fi
```

### Fish

Add to `~/.config/fish/config.fish`:

```fish
# Only run in zellij
if set -q ZELLIJ; and set -q ZELLIJ_PANE_ID
    function fish_prompt_precmd --on-event fish_prompt
        zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=idle" 2>/dev/null
    end

    function fish_preexec --on-event fish_preexec
        zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=working" 2>/dev/null
    end
end
```

## Custom Tool Integration

### Direct Pipe Messages

Any tool can send status updates using zellij's pipe command:

```bash
# Update by pane ID (requires ZELLIJ_PANE_ID)
zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=working"

# Update by tab name
zellij pipe --name zellij-crew:status --args "name=alice,state=attention"
```

### Python Example

```python
import os
import subprocess

def update_crew_status(state):
    """Update zellij-crew status for current pane."""
    pane_id = os.environ.get('ZELLIJ_PANE_ID')
    if not pane_id:
        return  # Not running in zellij

    subprocess.run([
        'zellij', 'pipe',
        '--name', 'zellij-crew:status',
        '--args', f'pane={pane_id},state={state}'
    ], stderr=subprocess.DEVNULL)

# Usage
update_crew_status('working')
# ... do work ...
update_crew_status('idle')
```

### Rust Example

```rust
use std::env;
use std::process::Command;

fn update_crew_status(state: &str) {
    let pane_id = match env::var("ZELLIJ_PANE_ID") {
        Ok(id) => id,
        Err(_) => return,  // Not running in zellij
    };

    let _ = Command::new("zellij")
        .args(&[
            "pipe",
            "--name", "zellij-crew:status",
            "--args", &format!("pane={},state={}", pane_id, state)
        ])
        .stderr(std::process::Stdio::null())
        .status();
}

// Usage
update_crew_status("working");
// ... do work ...
update_crew_status("idle");
```

## State Machine

Activity status follows this state machine:

```
                    ┌──────────┐
                    │ Unknown  │ (new tab)
                    └─────┬────┘
                          │
                          ▼
    ┌──────────────► ┌────────┐ ◄──────────────┐
    │                │  Idle  │                │
    │                └────┬───┘                │
    │                     │                    │
    │                     ▼                    │
    │                ┌─────────┐               │
    │   ┌────────────┤ Working ├──────────┐    │
    │   │            └─────────┘          │    │
    │   │                                 │    │
    │   ▼                                 ▼    │
    │ ┌──────────┐                  ┌──────────┐
    │ │ Question │                  │ Watching │
    │ └──────────┘                  └──────────┘
    │                                           │
    │   ┌───────────┐                          │
    └───┤ Attention │──────────────────────────┘
        └───────────┘

        ┌──────────┐
        │ Sleeping │ (timeout - not yet implemented)
        └──────────┘
```

**Typical flow:**
1. Tab opens → `unknown`
2. Claude/shell signals ready → `idle`
3. Work starts → `working`
4. Question asked → `question`
5. User answers → `working`
6. Work completes → `idle`
7. Needs user action → `attention`

## Troubleshooting

### Status not updating

1. Check zellij logs: `tail -f /tmp/zellij-*/zellij-log/zellij.log | grep crew`
2. Verify `ZELLIJ_PANE_ID` is set: `echo $ZELLIJ_PANE_ID`
3. Test manually: `zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=working"`
4. Check permissions: Crew needs `ReadCliPipes` permission

### Wrong tab updates

- By pane ID: Requires correct `ZELLIJ_PANE_ID` (auto-set by zellij)
- By name: Tab must have crew-assigned name (not "Tab #1")

### Hook script not found

```bash
# Check if in PATH
which zellij-crew-claude

# Or use absolute path in .claude/settings.json
"/full/path/to/zellij-crew/bin/zellij-crew-claude"
```

## Examples

### Build System Integration

```bash
#!/bin/bash
# build.sh - Show status during build

zellij-crew-claude working
cargo build --release

if [ $? -eq 0 ]; then
    zellij-crew-claude idle
else
    zellij-crew-claude attention
fi
```

### Test Runner

```bash
#!/bin/bash
# test-watch.sh - Monitor test runs

zellij-crew-claude watching
cargo watch -x test -s 'zellij-crew-claude idle'
```

### Jupyter Integration

```python
# Add to Jupyter notebook cell magics
from IPython.core.magic import register_line_magic

@register_line_magic
def crew(state):
    """Update zellij-crew status: %crew working"""
    import subprocess
    subprocess.run(['zellij-crew-claude', state])

# Usage in notebook:
# %crew working
# ... run expensive computation ...
# %crew idle
```

## Advanced: Name-Based Routing

Update tabs by their crew name instead of pane ID:

```bash
# Update "alice" tab
zellij pipe --name zellij-crew:status --args "name=alice,state=working"

# Update "bob" tab
zellij pipe --name zellij-crew:status --args "name=bob,state=attention"
```

This is useful for:
- Multi-agent coordination (one agent updates another's tab)
- Remote status updates (from scripts running outside zellij)
- Broadcast notifications (update all tabs named "monitor")

## Future: Message Passing

Planned feature for inter-agent communication:

```bash
# Send message from alice to bob
zellij pipe --name zellij-crew:message --args "from=alice,to=bob,msg=API schema ready"
```

See [DESIGN.md](DESIGN.md) Future Enhancements for details.
