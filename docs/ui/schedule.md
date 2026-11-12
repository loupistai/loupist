# Schedule

Bot’s More menu opens task history. Active and All filters, task details, run readers, linked evidence and notifications use the same saved task and run IDs. Focusing a control below the fold in a run scrolls it into the reader. A run's answer renders as the same card as in Chat, inside the run's own execution scope: its citations, links and workspace files open beside the briefing's own chat, which the Chat tab shows first (`useAppOpen`'s `openIn`), and a conflict card offers See on the Map. The reader lists watched assets and recent alerts. Looking ahead retains results and dividend record dates within seven calendar days of the saved run's start, including when that run is opened later. A task's detail offers Pause or Resume (Turn on for a proposed task), Run now and, for a custom task, Delete after a confirmation; these call `scheduled_task_set_status`, `scheduled_task_run_now` and `scheduled_task_delete` directly, so stopping a recurring model run never needs a model call. The briefing is turned off rather than deleted and a completed one-time task cannot be resumed. A failed last run says why beside those controls. The Active and All filter appears once a task exists. Creation and changes to a task's time or prompt start in conversation; every task detail says so and its Ask Bot link drafts the request in the composer when the shell provides one. A clear request registers immediately and returns the cadence and next execution time. A research session hands the request to its own Bot and stays open.

## Code

- Briefings and scheduled execution: `src-tauri/src/briefings/`, `src-tauri/src/scheduled_tasks/`, `src/components/briefings/`

## Process lifetime

See [Process lifetime](../runtime/scheduled-runs.md#process-lifetime) for resident
operation, shutdown and worker recovery.
