# Pipe Protocol Specification

This document specifies the pipe message protocols used by zellij-crew.

## Protocol Overview

Crew uses two separate pipe protocols:

1. **crew-state** (internal): Leader → Renderers state broadcast
2. **zellij-crew:status** (external): External tools → Leader activity updates

Both protocols use zellij's pipe messaging system (`zellij pipe` command).

---

# Protocol 1: crew-state (Internal)

**Purpose:** Leader broadcasts CrewTabState to all renderer instances

**Direction:** Leader → All Renderers (broadcast)

**Message Name:** `crew-state`

**Trigger:** Every TabUpdate event that changes tab state

## Message Format

**Payload:** JSON array of CrewTabState objects

```json
[
  {
    "tab_id": 1,
    "name": "alice",
    "status": "Working"
  },
  {
    "tab_id": 2,
    "name": "bob",
    "status": "Idle"
  }
]
```

**Note:** `pending_rename` field is skipped during serialization (`#[serde(skip)]`) - it's internal to the leader.

## Implementation Details

### Leader (Sender)

```rust
fn broadcast_state(&self) {
    let tabs: Vec<&CrewTabState> = self.known_tabs.values().collect();

    if let Ok(json) = serde_json::to_string(&tabs) {
        pipe_message_to_plugin(
            MessageToPlugin::new("crew-state")
                .with_plugin_url("crew")  // Routes to all instances with URL "crew"
                .with_payload(json)
        );
    }
}
```

**Key points:**
- Uses `with_plugin_url("crew")` WITHOUT `with_destination_plugin_id()` = broadcast
- All crew instances (leader + renderers) receive the message
- Instances filter by `is_leader` flag to determine handling

### Renderer (Receiver)

```rust
fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
    if !self.is_leader && pipe_message.name == "crew-state" {
        if let Some(payload) = pipe_message.payload {
            match serde_json::from_str::<Vec<CrewTabState>>(&payload) {
                Ok(tabs) => {
                    self.received_tabs = tabs;
                    return true;  // Request render
                }
                Err(e) => {
                    eprintln!("[crew:renderer] ERROR: Failed to parse state: {}", e);
                }
            }
        }
    }
    false
}
```

**Key points:**
- Renderers ignore message if `is_leader == true`
- Parse JSON into `Vec<CrewTabState>`
- Return `true` to trigger render with new state

## State Schema

### CrewTabState

| Field | Type | Description |
|-------|------|-------------|
| `tab_id` | u32 | Stable tab ID from "Tab #N" pattern |
| `name` | String | Current tab name ("alice" after rename, "Tab #5" before) |
| `status` | ActivityStatus | Current activity state (enum) |

### ActivityStatus Enum

Serialized as string variants:

```json
"Unknown"    // 🫥
"Idle"       // 🥱
"Working"    // 🤖
"Question"   // 🙋
"Sleeping"   // 😴
"Watching"   // 👀
"Attention"  // 🔔
```

## Broadcast Triggers

Leader broadcasts state when:
1. TabUpdate arrives (new tab, tab closed, tab renamed)
2. Activity status changes (from pipe message)
3. Name allocation completes

**Frequency:** Typically 0-5 times per second (only on state changes, not periodic)

---

# Protocol 2: zellij-crew:status (External)

**Purpose:** External tools update tab activity status

**Direction:** External → Leader (one-way)

**Message Name:** `zellij-crew:status`

**Source:** CLI pipes (`PipeSource::Cli`)

## Message Format

**Args:** Key-value pairs (comma-separated)

### Update by Pane ID

```bash
zellij pipe --name zellij-crew:status --args "pane=PANE_ID,state=STATE"
```

**Example:**
```bash
zellij pipe --name zellij-crew:status --args "pane=5,state=working"
```

### Update by Tab Name

```bash
zellij pipe --name zellij-crew:status --args "name=NAME,state=STATE"
```

**Example:**
```bash
zellij pipe --name zellij-crew:status --args "name=alice,state=attention"
```

### Help Command

```bash
zellij pipe --name zellij-crew:status --args "help"
```

Returns usage information via `cli_pipe_output()`.

### List Command

```bash
# Human-readable
zellij pipe --name zellij-crew:status --args "list"

# JSON format
zellij pipe --name zellij-crew:status --args "format=json,list"
```

Returns tab list via `cli_pipe_output()`.

## Valid States

| State | Case-sensitive | Default Indicator |
|-------|----------------|-------------------|
| `unknown` | yes | 🫥 |
| `idle` | yes | 🥱 |
| `working` | yes | 🤖 |
| `question` | yes | 🙋 |
| `sleeping` | yes | 😴 |
| `watching` | yes | 👀 |
| `attention` | yes | 🔔 |

Invalid states are rejected with error message.

**Note:** Default indicators can be overridden via plugin config keys `status_unknown`, `status_idle`, etc. Setting a key to `""` suppresses the `[brackets]` entirely for that state. See [DESIGN.md](DESIGN.md) for details.

## Implementation Details

### Leader (Receiver)

```rust
fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
    if self.is_leader && pipe_message.name == "zellij-crew:status" {
        // Help command
        if pipe_message.args.contains_key("help") {
            if let PipeSource::Cli(pipe_id) = &pipe_message.source {
                cli_pipe_output(pipe_id, HELP_TEXT);
            }
            return false;
        }

        // List command
        if pipe_message.args.contains_key("list") {
            // ... format and output tab list ...
            return false;
        }

        // Status update
        return self.handle_external_status_update(&pipe_message);
    }
    false
}
```

### Status Update Flow

**By Pane ID:**
1. Parse `pane=ID` from args
2. Look up pane in `pane_manifest` → get tab position
3. Map tab position to tab_id using `leader_tabs`
4. Update `known_tabs[tab_id].status`
5. Broadcast updated state to renderers

**By Name:**
1. Parse `name=NAME` from args
2. Find tab in `known_tabs` where `tab.name == NAME`
3. Update `tab.status`
4. Broadcast updated state to renderers

### Error Handling

| Error | Response |
|-------|----------|
| Invalid state | Log error, return false (no state change) |
| Pane not found | Log error with details (manifest contents) |
| Name not found | Log error "Tab 'NAME' not found" |
| Missing args | Log "Unrecognized format" |

## Response Format

### Help Command

Plain text output via `cli_pipe_output()`:

```
zellij-crew:status - Update tab activity status

Usage:
  zellij pipe --name zellij-crew:status --args "pane=PANE_ID,state=STATE"
  zellij pipe --name zellij-crew:status --args "name=NAME,state=STATE"

States:
  unknown   🫥  No status / agent exited
  idle      🥱  Agent idle
  working   🤖  Agent working
  question  🙋  Agent has a question
  sleeping  😴  Agent sleeping/paused
  watching  👀  Agent watching/monitoring
  attention 🔔  Needs attention

Config (in plugin KDL):
  status_unknown ""        Hide indicator when unknown
  status_working "WRK"     Custom text shown as [WRK]
  (set any status_* to "" to suppress the [brackets] entirely)
...
```

### List Command (Human-Readable)

Tab-separated table:

```
ID	Name	Status
--	----	------
1	alice	🤖 working
2	bob	🥱 idle
3	carol	🔔 attention
```

### List Command (JSON)

JSON array:

```json
[
  {
    "id": 1,
    "name": "alice",
    "status": "working"
  },
  {
    "id": 2,
    "name": "bob",
    "status": "idle"
  }
]
```

## Security Considerations

### No Authentication

Pipe messages are not authenticated. Any process running in the same zellij session can send status updates.

**Implications:**
- Malicious pane can set false status on other tabs
- Not suitable for trusted/untrusted status separation
- Acceptable for development tools in trusted environments

### No Encryption

Pipe message contents are not encrypted. Status information is visible to all processes.

### Rate Limiting

Not implemented. Leader processes all incoming messages.

**Potential DoS:** Flood leader with status updates. Mitigated by:
- Processing is O(1) per message (HashMap lookup)
- Broadcast only triggers render, zellij handles render rate limiting

## Performance Characteristics

### crew-state (Internal)

- **Frequency:** On state change only (0-5/sec typical)
- **Size:** ~100 bytes per tab (JSON overhead + tab data)
- **Latency:** <1ms (in-process pipe)
- **Scalability:** O(n) where n = number of tabs (typically < 20)

### zellij-crew:status (External)

- **Frequency:** User-dependent (typically 0-10/sec per pane)
- **Size:** ~50 bytes (args string)
- **Latency:** <5ms (CLI → leader)
- **Processing:** O(1) for name lookup, O(n) for pane lookup where n = panes in tab

## Future Extensions

### Planned: Inter-Agent Messaging

**Message Name:** `zellij-crew:message` (not yet implemented)

**Format:**
```bash
zellij pipe --name zellij-crew:message --args "from=alice,to=bob,msg=TEXT"
```

**Payload:**
```json
{
  "from": "alice",
  "to": "bob",
  "msg": "What's the API schema?",
  "timestamp": 1234567890
}
```

**Behavior:**
- Leader stores message in per-tab queue
- Target tab polls for messages (or receives notification)
- Show message indicator (📬) on tab bar

See [DESIGN.md](DESIGN.md) Future Enhancements for details.

## Protocol Versioning

**Current Version:** 1 (implicit, no version field)

**Compatibility:**
- Adding new states: Backward compatible (old clients ignore)
- Removing states: Breaking change (update all clients)
- New message names: Backward compatible (old clients ignore)
- Changing message format: Breaking change (coordinate update)

**Migration Strategy:** If breaking change needed, introduce new message name (e.g., `zellij-crew:status-v2`) and support both during transition period.

## Debugging

### Enable Debug Logging

Check zellij logs for pipe message details:

```bash
tail -f /tmp/zellij-*/zellij-log/zellij.log | grep -E 'crew|pipe'
```

**Useful patterns:**
- `Broadcasting state: N tabs` - Leader sent broadcast
- `Received state via pipe: N tabs` - Renderer received broadcast
- `Received message for 'NAME'` - Name-based routing
- `Updating tab 'NAME' to status:` - Status change applied
- `Tab 'NAME' not found` - Name routing failed
- `Pane N not found` - Pane routing failed

### Test Protocols Manually

**Test crew-state broadcast:**
- Create multiple tabs
- Watch logs for "Broadcasting state" messages
- Verify all renderers receive state

**Test status updates:**
```bash
# Send test update
zellij pipe --name zellij-crew:status --args "pane=$ZELLIJ_PANE_ID,state=working"

# Check logs
tail -f /tmp/zellij-*/zellij-log/zellij.log | grep "Updating tab"
```

**Test help command:**
```bash
zellij pipe --name zellij-crew:status --args "help"
```

**Test list command:**
```bash
zellij pipe --name zellij-crew:status --args "list"
zellij pipe --name zellij-crew:status --args "format=json,list"
```

## Reference Implementation

See source code:
- **Leader broadcast:** `src/main.rs:170-189` (broadcast_state)
- **Renderer receive:** `src/main.rs:577-596` (pipe() method, crew-state handling)
- **Status update:** `src/main.rs:321-457` (handle_external_status_update and helpers)
- **Help/List:** `src/main.rs:600-694` (pipe() method, zellij-crew:status handling)
