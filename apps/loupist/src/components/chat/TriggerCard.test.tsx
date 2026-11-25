import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TaskTrigger, TriggerScores } from "../../contracts/generated";
import type { ChatMessage, LoupistBridge } from "../../types";
import { ThreadDelivery } from "./ThreadDelivery";
import { TriggerCard } from "./TriggerCard";

const schedule = { kind: "interval", every_minutes: 45 } as const;
const baseline = { name: "price-band", version: 3, sha256: "ab".repeat(32) };
const approved = { ...baseline, at_ms: 1 };
type SavedScores = TriggerScores & { baseline?: typeof baseline };
const reviewedScores: SavedScores = { baseline, previous_met: 2, next_met: 1, compared: 4, days: [] };

const trigger = (patch: Partial<TaskTrigger> = {}): TaskTrigger => ({
  script: { name: "price-band", version: 3, sha256: "ab".repeat(32) },
  summary: "Prices outside a 5 percent band",
  tools: [{ name: "price" }, { name: "followed" }],
  args: { band_percent: 5 },
  state: "proposed",
  script_state: {},
  evaluations: [],
  counters: { today: { day: "2026-11-24", evaluations: 1, reads: 2, met: 0 }, recent: [], agent_runs_today: 0, error_streak: 0, no_data_streak: 0, backoff_ms: 0, reads_blocked: false },
  approved_versions: [],
  stale_after_days: 5,
  reads_per_check: 4,
  proposal_test: {
    today_met: false, today_reading: "Northwind −1.2%", today_outcome: "not_met",
    fired_on: ["2026-11-12", "2026-11-24"], replayed: 18, unreplayed: 2,
    days: [{ date: "2026-11-12", outcome: "met", reading: "Northwind +6.2%" }, { date: "2026-11-24", outcome: "met", reading: "Northwind −7.4%" }],
    reads: 8,
    scores: reviewedScores,
  },
  ...patch,
});

function bridge(extra: Partial<LoupistBridge> = {}): LoupistBridge {
  return {
    scheduledTriggerGet: async ({ version }) => ({ code: "import loupist\n", summary: "Prices", version: version ?? 3, sha256: (version === 4 ? "cd" : "ab").repeat(32), task: {} as never }),
    scheduledTriggerApprove: async () => ({ id: "task" } as never),
    scheduledTriggerTest: async () => ({ at_ms: 1, scheduled_for_ms: 1, outcome: "not_met", reading: "No asset beyond the band", evidence: {}, version: 3, sha256: "ab".repeat(32), reads: 1, coalesced: false, ledger: [] }),
    scheduledTriggerRevert: async () => ({ id: "task" } as never),
    ...extra,
  } as LoupistBridge;
}

it("shows the dry run, both versions' scores, and the code", async () => {
  const user = userEvent.setup();
  render(<TriggerCard trigger={trigger({ state: "update_proposed", approved, pending: { version: 4, sha256: "cd".repeat(32), test: trigger().proposal_test } })} bridge={bridge()} loupistId="desk" taskId="task" schedule={schedule} revision={7} prompt="Report the move." />);
  expect(screen.getByText(/Checked on today's data: not met/)).toBeVisible();
  expect(screen.getByText(/2 days could not be replayed/)).toBeVisible();
  expect(screen.getByText(/Nov 12|2026-11-12/)).toBeVisible();
  expect(screen.getByText(/approved version was met 2 times/)).toBeVisible();
  expect(screen.getByRole("button", { name: "Update to v4" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "View code" }));
  expect(await screen.findByText(/import loupist/)).toBeVisible();
});

it("labels checks with a missing replay baseline and hides empty scores", () => {
  const test = trigger().proposal_test!;
  const skipped = { date: "2026-11-24", previous: "met", next: "no_data" } as const;
  const card = (scores: SavedScores) => <TriggerCard
    trigger={trigger({ state: "update_proposed", approved, pending: {
      version: 4, sha256: "cd".repeat(32), test: { ...test, scores },
    } })} bridge={bridge()} loupistId="desk" taskId="task" schedule={schedule} revision={7} />;
  const { rerender } = render(card({ baseline, previous_met: 0, next_met: 0, compared: 0, days: [skipped] }));
  expect(screen.getByText("1 stored check is not comparable.")).toBeVisible();
  expect(screen.queryByText(/approved version was met/)).toBeNull();
  rerender(card({
    baseline, previous_met: 1, next_met: 1, compared: 1,
    days: [skipped, { date: "2026-11-23", previous: "met", next: "met" }],
  }));
  expect(screen.getByText("1 stored check is not comparable.")).toBeVisible();
  expect(screen.getByText(/The approved version was met 1 times; the new one 1, of 1 stored checks/)).toBeVisible();
});

it("hides pooled scores, scores for another approved pin, and scores after approval", () => {
  const test = trigger().proposal_test!;
  const { baseline: _baseline, ...unscoped } = reviewedScores;
  const pending = (scores: SavedScores) => ({
    version: 4, sha256: "cd".repeat(32), test: { ...test, scores },
  });
  const card = (check: TaskTrigger) => <TriggerCard trigger={check} bridge={bridge()}
    loupistId="desk" taskId="task" schedule={schedule} revision={7} />;
  const { rerender } = render(card(trigger({
    approved, state: "update_proposed", pending: pending(unscoped),
  })));
  expect(screen.queryByText(/approved version was met/)).toBeNull();
  rerender(card(trigger({
    approved, state: "update_proposed", pending: pending({ ...unscoped, baseline: { ...baseline, sha256: "ef".repeat(32) } }),
  })));
  expect(screen.queryByText(/approved version was met/)).toBeNull();
  rerender(card(trigger({ approved, state: "on" })));
  expect(screen.queryByText(/approved version was met/)).toBeNull();
});

it("clears the reviewed code when the displayed version changes and binds approval to the new revision", async () => {
  const user = userEvent.setup();
  const approve = vi.fn().mockResolvedValue({ id: "task" });
  const api = bridge({ scheduledTriggerApprove: approve });
  const card = (version: number, revision: number) => <TriggerCard
    trigger={trigger({ script: { name: "price-band", version, sha256: "ab".repeat(32) } })}
    bridge={api} loupistId="desk" taskId="task" schedule={schedule} revision={revision}
  />;
  const { rerender } = render(card(2, 7));
  await user.click(screen.getByRole("button", { name: "View code" }));
  expect(await screen.findByText(/import loupist/)).toBeVisible();
  rerender(card(3, 8));
  expect(screen.queryByText(/import loupist/)).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Turn on" }));
  expect(approve).toHaveBeenCalledExactlyOnceWith({
    loupist_id: "desk", task_id: "task", expected_revision: 8, version: 3, sha256: "ab".repeat(32),
  });
});

it("requires a successful dry run for the pending arguments and providers before enabling approval", () => {
  const pending = { version: 4, sha256: "cd".repeat(32) };
  const api = bridge();
  const { rerender } = render(<TriggerCard trigger={trigger({ state: "update_proposed", pending })} bridge={api} loupistId="desk" taskId="task" schedule={schedule} revision={7} />);
  expect(screen.getByRole("button", { name: "Update to v4" })).toBeDisabled();
  rerender(<TriggerCard trigger={trigger({ state: "update_proposed", pending: {
    ...pending, test: trigger().proposal_test,
    configuration: { summary: "Check for an eight percent move", args: { band_percent: 8 }, tools: [{ name: "price", provider: "provider-b" }], reads_per_check: 1 },
  } })} bridge={api} loupistId="desk" taskId="task" schedule={schedule} revision={8} />);
  expect(screen.getByText("Check for an eight percent move")).toBeVisible();
  expect(screen.queryByText("Prices outside a 5 percent band")).not.toBeInTheDocument();
  expect(screen.getByText("band_percent")).toBeVisible();
  expect(screen.getByText("8", { selector: "code" })).toBeVisible();
  expect(screen.getByText("price (provider-b)")).toBeVisible();
  expect(screen.getByRole("button", { name: "Update to v4" })).toBeEnabled();
});

it("requests a test only for the displayed pending revision and hash", async () => {
  const user = userEvent.setup();
  const testNow = vi.fn().mockRejectedValue(new Error("The check proposal changed"));
  const pending = { version: 4, sha256: "cd".repeat(32) };
  render(<TriggerCard trigger={trigger({ state: "update_proposed", pending })} bridge={bridge({ scheduledTriggerTest: testNow })}
    loupistId="desk" taskId="task" schedule={schedule} revision={7} />);
  await user.click(screen.getByRole("button", { name: "Test now" }));
  expect(testNow).toHaveBeenCalledExactlyOnceWith({ loupist_id: "desk", task_id: "task", expected_revision: 7, ...pending });
  expect(await screen.findByRole("alert")).toBeVisible();
});

it("projects daily reads from the live check cost and cadence", () => {
  render(<TriggerCard trigger={trigger()} bridge={bridge()} loupistId="desk" taskId="task" schedule={schedule} revision={7} />);
  expect(screen.getByText("About 128 data reads a day")).toBeVisible();
});

it("shows reads for each run day without multiplying by the scheduled weekdays", () => {
  render(<TriggerCard trigger={trigger({ reads_per_check: 2 })} bridge={bridge()}
    loupistId="desk" taskId="task"
    schedule={{ kind: "weekly", days: [1, 2, 3, 4, 5], hour: 9, minute: 0, timezone: "UTC" }}
    revision={7} />);
  expect(screen.getByText("About 2 data reads per run day")).toBeVisible();
});

it("uses the full active interval window on its run day", () => {
  render(<TriggerCard trigger={trigger({ reads_per_check: 2, active_hours: {
    days: [1], start: "09:00", end: "17:00", timezone: "UTC",
  } })} bridge={bridge()} loupistId="desk" taskId="task"
    schedule={{ kind: "interval", every_minutes: 60 }} revision={7} />);
  expect(screen.getByText("About 16 data reads per run day")).toBeVisible();
});

it("shows a failing check and a result that is only the reading", () => {
  render(<TriggerCard trigger={trigger({ state: "failing", last: { at_ms: 1, scheduled_for_ms: 1, outcome: "error", reading: "", evidence: {}, version: 2, sha256: "ab".repeat(32), reads: 0, error: "NameError: points", coalesced: false, ledger: [] }, approved_versions: [{ version: 2, sha256: "aa".repeat(32), at_ms: 1 }, { version: 3, sha256: "ab".repeat(32), at_ms: 2 }] })} bridge={bridge()} loupistId="desk" taskId="task" schedule={schedule} revision={7} />);
  expect(screen.getByText(/stopped working: NameError: points/)).toBeVisible();
  expect(screen.getByRole("button", { name: "Go back" })).toBeVisible();
  const message: ChatMessage = {
    id: "delivery", role: "assistant", status: "complete", created_at: new Date().toISOString(),
    content: "Northwind fell 7.4%.",
    delivery: {
      run_id: "run", task_id: "task", task_title: "Price moves", task_kind: "custom", status: "running",
      schedule: null, run_thread_id: null, finished_at_ms: Date.now(),
      trigger: { reading: "Northwind −7.4%", as_of: "2026-11-24", sources: ["price"], checked_at_ms: Date.now(), explained: false, reason: "No AI account is connected.", coalesced: true },
    },
  };
  render(<ThreadDelivery message={message} loupistName="Research" />);
  expect(screen.getByText(/From your check: Northwind.* · price/)).toBeVisible();
  expect(screen.getByText(/No explanation/)).toBeVisible();
  expect(screen.getByText(/after Loupist was closed/)).toBeVisible();
});
