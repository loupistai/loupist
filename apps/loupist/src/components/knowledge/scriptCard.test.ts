import type { ScriptSummary } from "../../contracts/generated";
import type { ScheduledTaskListItem } from "../../contracts/scheduled";
import { folded, oneReading, scriptReads, scriptTask } from "./scriptCard";

const script = (extra: Partial<ScriptSummary> = {}): ScriptSummary => ({
  name: "margins", version: 3, sha256: "", createdAtMs: 1, usedBy: ["Morning check"],
  metadata: { title: "Quarterly margins", summary: "Quarterly margins", args: {}, origin: { kind: "user" }, afterOutsideText: false, reads: [
    { tool: "equity_quote", status: "ok" }, { tool: "filings", status: "ok" }, { tool: "news", status: "ok" }, { tool: "prices", status: "ok" },
  ] },
  ...extra,
});

it("folds a summary that only differs by case and spaces", () => {
  expect(folded("Quarterly   Margins")).toBe(folded("quarterly margins"));
  expect(folded("Count quarters.")).not.toBe(folded("quarters"));
});

it("names three reads and counts the rest", () => {
  expect(scriptReads(script())).toEqual({ labels: ["Equity quote", "Filings", "News"], extra: 1 });
});

it("keeps a reading to one line", () => {
  expect(oneReading("Margin held.\nMore")).toBe("Margin held.");
  expect([...oneReading("m".repeat(90))].length).toBeLessThanOrEqual(80);
});

it("links the task whose check names the script", () => {
  const tasks = [
    { id: "other", title: "Other", trigger: { script: { name: "other", version: 1, sha256: "" } } },
    { id: "task", title: "Margin check", trigger: { script: { name: "margins", version: 3, sha256: "" }, last: { outcome: "met", reading: "Margin held.", at_ms: 1 } } },
  ] as ScheduledTaskListItem[];
  expect(scriptTask(script(), tasks)?.id).toBe("task");
  expect(scriptTask(script(), [])).toBeUndefined();
});
