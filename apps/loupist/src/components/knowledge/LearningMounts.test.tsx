import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ScheduledRun, ScheduledTask } from "../../types";
import { PageReader } from "../memory/PageReader";
import { OpenProvider } from "../navigation/Open";
import { TooltipProvider } from "../ui/tooltip";
import { LensLearningPanel } from "./LensLearningPanel";

// The seeded TCP page's overview and risks sections were filed by this message.
const FILED = "mock-filed-answer";

function learningProgram(loupistId: string): [ScheduledTask, ScheduledRun[]] {
  const task = {
    id: "learn-santos", loupist_id: loupistId, kind: "learning", title: "Porto de Santos", prompt: "Track soybean shipments through Santos.",
    schedule: { kind: "daily", hour: 19, minute: 0, timezone: "America/Sao_Paulo" }, target: { kind: "dedicated_chat" }, model_profile_id: "", thinking_level: "", run_options: {},
    use_memory: true, update_memory: true, status: "active", revision: 1, next_run_at_ms: null, last_run_at_ms: null, created_at_ms: 0, updated_at_ms: 0, last_error: null,
    session_thread_id: null, awaiting_confirmation: false,
  } as ScheduledTask;
  const run = {
    quiet: false, briefing_context: null, kind: "learning", summary: "", read_at_ms: null, message_id: FILED, skipped_tools: [], id: "run-santos-1", task_id: task.id,
    loupist_id: loupistId, task_revision: 1, task_title: task.title, thread_id: null, run_id: "run-santos-1", scheduled_for_ms: 0,
    // The chip prints the local calendar day. Noon UTC is 20 Oct from UTC-11 through UTC+11; 22:00 UTC is the next day in Korea.
    started_at_ms: Date.UTC(2026, 9, 20, 12), finished_at_ms: Date.UTC(2026, 9, 20, 12, 5), status: "succeeded", error: null,
  } as ScheduledRun;
  return [task, [run]];
}

it("marks the sections a learning run filed in the page reader, and opens that run", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0]!.id;
  bridge.installScheduledTask(...learningProgram(loupistId));
  const open = vi.fn(async () => {});
  render(<TooltipProvider><OpenProvider open={open}><PageReader bridge={bridge} loupistId={loupistId} recordId="lens-subject-tcp" catalog={await bridge.knowledgeCatalog({ loupistId })} onOpen={vi.fn()} onOpenPage={vi.fn()} onAsk={vi.fn()} onChanged={vi.fn()} /></OpenProvider></TooltipProvider>);
  const page = await screen.findByRole("article", { name: "Page" });
  const overview = page.querySelector<HTMLElement>('[data-section-key="overview"]')!;
  const chip = await within(overview).findByRole("link", { name: "Learned Oct 20 · Porto de Santos" });
  expect(page.querySelector('[data-section-key="facts"] [data-chip="learned"]')).toBeNull();
  await userEvent.setup().click(chip);
  expect(open).toHaveBeenCalledWith({ kind: "run", runId: "run-santos-1", taskId: "learn-santos" });
});

it("says why method learning waits in the Lens learning panel of a method", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0]!.id;
  render(<LensLearningPanel bridge={bridge} loupistId={loupistId} node={{ kind: "lens_method", recordId: "lens-method-x" }} onArchived={vi.fn()} />);
  expect(await screen.findByText("A method is learned once a second, separate case shows it.")).toBeVisible();
});
