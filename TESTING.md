# Manual Testing Procedure

## Setup

1. Build the plugin:
   ```bash
   cargo build --release
   ```

2. Create a test layout file (`test-layout.kdl`):
   ```kdl
   layout {
       pane size=1 borderless=true {
           plugin location="file:target/wasm32-wasip1/release/zellij_crew.wasm" {
               names "alice bob carol dave eve"
               mode "round-robin"
           }
       }
       pane size=1 borderless=true {
           plugin location="status-bar"
       }

       tab  # CRITICAL: Explicit tab for session-level loading
   }
   ```

3. Start zellij with the test layout:
   ```bash
   zellij --layout test-layout.kdl
   ```

4. Grant permission when prompted (focus the plugin pane and approve).

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

### T8: Verify Single Instance

1. Start with test layout
2. Check logs: `tail -f /tmp/zellij-*/zellij-log/zellij.log | grep crew`
3. Create multiple tabs
4. Expected: All log lines show SAME plugin ID (e.g., `[crew:1]`)
5. If multiple IDs appear (1, 6, 8), layout is broken (per-tab loading)

## Debugging

View plugin stderr:
```bash
# In another terminal
tail -f /tmp/zellij-*/zellij-log/zellij.log
```

Or check zellij's stderr output directly if running in foreground.
