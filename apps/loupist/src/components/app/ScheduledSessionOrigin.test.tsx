import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "@/bridge/mockBridge";
import { formatDate } from "@/i18n/format";
import { scheduledRunFixture } from "@/test/scheduledFixtures";
import { OpenProvider } from "../navigation/Open";
import { ScheduledSessionOrigin } from "./ScheduledSessionOrigin";

const loupistId = "loupist-quality";
const date = "2026-11-06T08:00:00Z";
it.each(["fresh", "shared", "pruned", "deleted"] as const)("attributes a %s scheduled session to the retained record", async kind => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const task = await bridge.proposeScheduledTask({
    loupist_id: loupistId, title: "Chip filings", prompt: "Find updates", schedule: { kind: "once", at_ms: 1000 },
    target: { kind: kind === "shared" ? "dedicated_chat" : "new_chat" }, model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: true, status: "active",
  });
  const run = scheduledRunFixture({ task_id: task.id });
  const readTask = vi.spyOn(bridge, "scheduledTaskGet").mockResolvedValue(kind === "deleted" ? null : task);
  const readRuns = vi.spyOn(bridge, "scheduledTaskRuns").mockResolvedValue(kind === "pruned" ? [] : [run]);
  const open = vi.fn().mockResolvedValue(undefined);
  const origin = { turn: { task_id: task.id, run_id: run.run_id, task_kind: task.kind, task_title: task.title }, date, shared: false };
  render(<OpenProvider open={open}><ScheduledSessionOrigin bridge={bridge} loupistId={loupistId} origin={origin} /></OpenProvider>);
  const label = "From Schedule: Chip filings" + (kind === "shared" || kind === "deleted" ? "" : " · run of " + formatDate(Date.parse(date)));
  await waitFor(() => expect(readTask).toHaveBeenCalled());
  if (kind === "fresh" || kind === "shared") {
    await userEvent.click(await screen.findByRole("button", { name: label }));
    expect(open).toHaveBeenCalledWith(kind === "shared" ? { kind: "schedule", taskId: task.id } : { kind: "run", taskId: task.id, runId: run.run_id });
  } else {
    expect(await screen.findByTitle(label)).toBeVisible();
    expect(screen.queryByRole("button")).toBeNull();
  }
  expect(readRuns.mock.calls.length > 0).toBe(kind === "fresh" || kind === "pruned");
});
