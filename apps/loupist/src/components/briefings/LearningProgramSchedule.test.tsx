import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MOCK_LOUPISTS } from "../../bridge/mock/fixtures";
import type { ScheduledTask } from "@/types";
import { ScheduleView } from "./ScheduleView";

const loupist = MOCK_LOUPISTS[0]!;
const program = {
  id: "learn-nordic", loupist_id: loupist.id, kind: "learning", title: "Nordic hydropower reservoirs", prompt: "Weekly reservoir levels and their effect on power prices.",
  schedule: { kind: "weekly", days: [1, 4], hour: 7, minute: 0, timezone: "Europe/Oslo" }, target: { kind: "dedicated_chat" }, model_profile_id: "", thinking_level: "", run_options: {},
  use_memory: true, update_memory: true, status: "active", revision: 3, next_run_at_ms: null, last_run_at_ms: null, created_at_ms: 0, updated_at_ms: 0, last_error: null,
  session_thread_id: null, awaiting_confirmation: false,
  program: { depth: "standard", scope: { connections: "all_enabled", web: true }, keeps: { sources: true, lens: true, asset_reads: false } },
  subjects: [{ name: "Nord Pool" }], report: "when_new", delivery: { to: "bot" },
} as ScheduledTask;

/** The native case "edit depth and time, relaunch, both persist" drives these same selectors. */
it("edits a program's depth and time from its Schedule detail, and the detail shows both", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  bridge.installScheduledTask(program);
  const user = userEvent.setup();
  render(<ScheduleView bridge={bridge} loupist={loupist} theme="light" open={vi.fn()} onOpenChat={vi.fn()} initialTaskId="learn-nordic" />);
  const detail = await screen.findByRole("article", { name: "Nordic hydropower reservoirs" });
  await user.click(within(detail).getByRole("button", { name: "Edit" }));
  const editor = await screen.findByRole("dialog", { name: "Edit learning program" });
  expect(editor).toHaveAttribute("data-program-editor");
  await user.click(within(editor).getByRole("radio", { name: "Deep" }));
  await user.click(within(editor).getByRole("combobox", { name: "Time" }));
  await user.click(await screen.findByRole("option", { name: "9:15 PM" }));
  await user.click(within(editor).getByRole("button", { name: "Save" }));
  expect(screen.queryByRole("dialog", { name: "Edit learning program" })).toBeNull();
  const fields = within(await screen.findByRole("article", { name: "Nordic hydropower reservoirs" }));
  expect(await fields.findByText("Deep")).toBeVisible();
  expect(fields.getAllByText(/9:15\s?PM/).length).toBeGreaterThan(0);
  expect(await bridge.scheduledTaskGet({ loupist_id: loupist.id, task_id: "learn-nordic" })).toMatchObject({ revision: 4, program: { depth: "deep" }, schedule: { hour: 21, minute: 15 } });
});
