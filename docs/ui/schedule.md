# Schedule

Bot’s More menu opens task history. Active and All filters, task details, run readers, linked evidence and notifications use the same saved task and run IDs. Focusing a control below the fold in a run scrolls it into the reader. The reader lists watched assets and recent alerts. Creation, edits and cancellation start in conversation. A clear request registers immediately and returns the cadence and next execution time. A research session hands the request to its own Bot and stays open.

## Code

- Briefings and scheduled execution: `src-tauri/src/briefings/`, `src-tauri/src/scheduled_tasks/`, `src/components/briefings/`

## Process lifetime

World jobs and World search run in-process through Rust and the local embedding
model. Existing folders keep their canonical SQLite snapshot; leftover
`graph-instance/` data is ignored and never migrated or read.
App exit stops schedule admission, cancels/drains World work and shuts down MCP
workers and leased processes. The Tauri exit callback allows 30 seconds; unfinished
cleanup retains leases for startup recovery. Recovery verifies process identity
before signaling. Process leases are implemented for macOS and Windows; other Unix
hosts fail closed with `UnsupportedPlatform`.

Keep running in the menu bar for briefings defaults on, can be
turned off in the briefing schedule editor, and turning a briefing on sets it again. Closing the main window hides it while
that setting is on.
Menu Quit and Cmd+Q exit. The tray menu opens the app, selects a Loupist's inbox,
or quits. A finished briefing posts an OS notification when permission is
granted. That notification does not open the item. Tray selection emits the
typed `inbox-open` event. Schedules run only while the app process is running,
including while the window is hidden.
