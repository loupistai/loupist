import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { BotMemoryListEntry, UsageSummary } from "../../contracts/generated";
import type { LoupistBridge } from "../../types";
import { StandingSheet } from "./StandingSheet";

const preference: BotMemoryListEntry = {
  id: "bm-pref", kind: "preference", text: "Answer in two lines", sinceMs: Date.UTC(2026, 10, 1), updatedMs: Date.UTC(2026, 10, 1),
};
const decision: BotMemoryListEntry = {
  id: "bm-dec", kind: "decision", text: "Wait on the basket", sinceMs: Date.UTC(2026, 10, 2), updatedMs: Date.UTC(2026, 10, 3),
};

function usage(programs: UsageSummary["programs"]): UsageSummary {
  const totals = { runs: 0, unknownRuns: 0, requests: 0, inputTokens: 0, cachedInputTokens: 0, outputTokens: 0, costEstimate: null };
  return { fromMs: 0, toMs: 1, totals, accounts: [], days: [], surfaces: [], loupists: [], programs };
}

function bridge(entries = [preference, decision]) {
  const list = [...entries];
  const botMemoryList = vi.fn(async () => list.map(entry => ({ ...entry })));
  const botMemoryForget = vi.fn(async ({ id }: { id: string }) => {
    const next = list.filter(entry => entry.id !== id);
    list.splice(0, list.length, ...next);
    return next.map(entry => ({ ...entry }));
  });
  const botMemoryUpdate = vi.fn(async ({ id, text }: { id: string; text: string }) => {
    const entry = list.find(item => item.id === id);
    if (!entry) throw new Error("missing");
    entry.text = text.trim();
    entry.updatedMs = entry.sinceMs + 86_400_000;
    return list.map(item => ({ ...item }));
  });
  const usageSummary = vi.fn(async () => usage([
    { loupistId: "loupist-a", taskId: "task-brief", title: "Morning briefing", runs: 4, totals: { ...usage([]).totals, runs: 4, requests: 9 } },
    { loupistId: "other", taskId: "task-other", title: "Someone else", runs: 2, totals: { ...usage([]).totals, runs: 2, requests: 3 } },
    { loupistId: "loupist-a", taskId: "task-gone", title: null, runs: 1, totals: { ...usage([]).totals, runs: 1, requests: 1 } },
  ]));
  return { bridge: { botMemoryList, botMemoryForget, botMemoryUpdate, usageSummary } as unknown as LoupistBridge, botMemoryList, botMemoryForget, botMemoryUpdate, usageSummary };
}

it("lists every entry, edits and forgets without a model call, and shows this Loupist's task usage", async () => {
  const user = userEvent.setup();
  const api = bridge();
  const onClose = vi.fn();
  render(<StandingSheet open loupistId="loupist-a" bridge={api.bridge} onClose={onClose} />);
  const sheet = await screen.findByRole("dialog", { name: "Standing instructions" });
  expect(sheet).toHaveTextContent("Answer in two lines");
  expect(sheet).toHaveTextContent("Wait on the basket");
  expect(sheet).toHaveTextContent("Preference");
  expect(sheet).toHaveTextContent("Decision");
  expect(await within(sheet).findByText("Morning briefing")).toBeVisible();
  expect(sheet).toHaveTextContent("Removed task");
  expect(sheet).not.toHaveTextContent("Someone else");

  await user.click(within(sheet).getAllByRole("button", { name: "Edit" })[0]!);
  const field = within(sheet).getByRole("textbox", { name: "Entry text" });
  await user.clear(field);
  await user.type(field, "Answer in one line");
  await user.click(within(sheet).getByRole("button", { name: "Save" }));
  expect(api.botMemoryUpdate).toHaveBeenCalledWith({ loupistId: "loupist-a", id: "bm-pref", text: "Answer in one line" });
  expect(sheet).toHaveTextContent("Answer in one line");
  expect(sheet).toHaveTextContent("Preference");

  await user.click(within(sheet).getAllByRole("button", { name: "Forget" })[0]!);
  expect(api.botMemoryForget).toHaveBeenCalledWith({ loupistId: "loupist-a", id: "bm-pref" });
  expect(sheet).not.toHaveTextContent("Answer in one line");
  expect(sheet).toHaveTextContent("Wait on the basket");

  await user.click(within(sheet).getByRole("button", { name: "Close standing instructions" }));
  expect(onClose).toHaveBeenCalledOnce();
});

it("says when nothing is kept and when task usage is empty", async () => {
  const api = bridge([]);
  api.usageSummary.mockResolvedValue(usage([]));
  render(<StandingSheet open loupistId="loupist-a" bridge={api.bridge} onClose={vi.fn()} />);
  const sheet = await screen.findByRole("dialog", { name: "Standing instructions" });
  expect(await within(sheet).findByText("Nothing kept yet. Tell the Bot in the conversation.")).toBeVisible();
  expect(await within(sheet).findByText("No task usage in the last 30 days.")).toBeVisible();
});
