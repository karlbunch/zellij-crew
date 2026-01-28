# Manual Testing Procedure

## Setup

1. Build the plugin:
   ```bash
   cargo build --release
   ```

2. Add leader config to `~/.config/zellij/config.kdl`:
   ```kdl
   plugins {
       crew location="file:target/wasm32-wasip1/release/zellij-crew.wasm" {
           names "alice bob carol dave eve"
           mode "round-robin"
       }
   }

   load_plugins {
       "crew"  // Leader instance (background)
   }
   ```

3. Create a test layout file (`~/.config/zellij/layouts/crew-test.kdl`):
   ```kdl
   layout {
       default_tab_template {
           pane size=1 borderless=true {
               plugin location="crew"  // Renderer per tab
           }
           children
           pane size=1 borderless=true {
               plugin location="status-bar"
           }
       }

       tab
   }
   ```

4. Start zellij with the test layout:
   ```bash
   zellij --layout crew-test
   ```

5. Grant permissions when prompted (leader instance will show permission dialog).

## Test Cases

### T1: Basic Naming (round-robin)

1. Open zellij with a single tab
2. Expected: Tab renamed to "alice"
3. Create new tab (Ctrl+t, n)
4. Expected: New tab named "bob"
5. Create another tab
6. Expected: New tab named "carol"

### T2: Tab Closure (round-robin)

1. With tabs: alice, bob, carol
2. Close the "bob" tab (middle tab)
3. Create new tab
4. Expected: New tab named "dave" (NOT "bob" - round-robin continues forward)

### T3: Fill-in Mode

1. Restart with `mode "fill-in"` in config
2. Create 3 tabs: alice, bob, carol
3. Close "bob"
4. Create new tab
5. Expected: New tab named "bob" (fills the gap)

### T4: Show Position

1. Restart with `show_position "true"`
2. Create tabs
3. Expected: Names like "alice <1>", "bob <2>", etc.

### T5: Custom Tab Names

1. Start with `rename_custom "false"` (default)
2. Manually rename a tab to "mywork" (Ctrl+t, r, type name)
3. Expected: Tab keeps name "mywork"

4. Restart with `rename_custom "true"`
5. Tab should be renamed to a pool name

### T6: Pool Exhaustion

1. Configure with short pool: `names "a b c"`
2. Create 4 tabs
3. Expected: First 3 tabs named a, b, c; 4th tab unnamed (keeps default "Tab #4")

### T7: Existing Tabs on Load

1. Start zellij without plugin, create several tabs with default names
2. Load the plugin
3. Expected: All existing tabs with default names get renamed

### T8: Verify Leader/Renderer Architecture

1. Start with test layout
2. Check logs: `tail -f /tmp/zellij-*/zellij-log/zellij.log | grep crew`
3. Create multiple tabs
4. Expected: Multiple plugin IDs - one leader (usually id: 1), multiple renderers (id: 2, 4, 6...)
5. Leader logs show: `is_leader=true`, `rows=20` on first render
6. Renderer logs show: `is_leader=false`, `rows=0` or `rows=1` on first render
7. Only leader logs show "Broadcasting state" messages
8. All renderers receive "Received state via pipe" messages

### T9: Activity Status Update (Pane ID)

1. Start zellij with crew
2. In a terminal pane: `echo $ZELLIJ_PANE_ID` (note the ID)
3. Update status: `zellij pipe --name zellij-crew:status --args "pane=ID,state=working"`
4. Expected: Tab shows 🤖 indicator
5. Change status: `zellij pipe --name zellij-crew:status --args "pane=ID,state=attention"`
6. Expected: Tab shows 🔔 indicator

### T10: Activity Status Update (Name)

1. Create tabs with crew names (alice, bob, carol)
2. Update by name: `zellij pipe --name zellij-crew:status --args "name=alice,state=question"`
3. Expected: Alice tab shows 🙋 indicator
4. Switch to different tab, verify indicator persists

### T11: Hook Script

1. Ensure `bin/zellij-crew-claude` is in PATH or use absolute path
2. Run: `zellij-crew-claude working`
3. Expected: Current tab shows 🤖 indicator
4. Run: `zellij-crew-claude idle`
5. Expected: Current tab shows 🥱 indicator

### T12: Status Query Commands

1. Create several tabs with different statuses
2. Run: `zellij pipe --name zellij-crew:status --args help`
3. Expected: Help text showing all states and usage
4. Run: `zellij pipe --name zellij-crew:status --args list`
5. Expected: Table showing all tabs with their current status
6. Run: `zellij pipe --name zellij-crew:status --args "format=json,list"`
7. Expected: JSON array with tab objects

## Debugging

View plugin stderr:
```bash
# In another terminal
tail -f /tmp/zellij-*/zellij-log/zellij.log
```

Or check zellij's stderr output directly if running in foreground.

**Useful log patterns:**
- `grep "crew:leader"` - Leader instance logs only
- `grep "crew:renderer"` - Renderer instance logs only
- `grep "Broadcasting state"` - State broadcasts from leader
- `grep "Received state via pipe"` - State received by renderers
- `grep "Updating tab.*to status"` - Status changes
