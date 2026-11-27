import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, test, vi } from "vitest";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ScheduledTaskListItem } from "../../contracts/scheduled";
import { OpenProvider } from "../navigation/Open";
import { ScriptLibrary } from "./ScriptLibrary";

test("the library saves edits as new versions and restores by making another version", async () => {
  const bridge = new MockLoupistBridge();
  await bridge.scriptSave({ loupistId: "research", draft: {
    name: "quarterly-margin", title: "Quarterly margins", summary: "Summarize the results.", code: "print(1)",
  } });
  render(<ScriptLibrary bridge={bridge} loupistId="research" query="" onQueryChange={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: /Quarterly margins/ }));
  fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Python code" }), { target: { value: "print(2)" } });
  fireEvent.click(screen.getByRole("button", { name: "Save new version" }));
  expect(await screen.findByRole("heading", { name: "Quarterly margins · v2" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "v1" }));
  expect(await screen.findByText("print(1)")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Restore as a new version" }));
  expect(await screen.findByRole("heading", { name: "Quarterly margins · v3" })).toBeInTheDocument();
  expect((await bridge.scriptGet({ loupistId: "research", name: "quarterly-margin", version: 2 })).code).toBe("print(2)");
});

test("a script card shows its version, reads, linked task and last result", async () => {
  const bridge = new MockLoupistBridge();
  await bridge.scriptSave({ loupistId: "research", draft: {
    name: "margins", title: "Quarterly review", summary: "Quarterly review", code: "print(1)",
  } });
  const saved = await bridge.scriptGet({ loupistId: "research", name: "margins" });
  saved.summary.metadata.reads = [
    { tool: "equity_quote", status: "ok" }, { tool: "filings", status: "ok" }, { tool: "news", status: "ok" }, { tool: "prices", status: "ok" },
  ];
  vi.spyOn(bridge, "scriptsList").mockResolvedValue([saved.summary]);
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([{
    id: "task", title: "Margin check", trigger: {
      script: { name: "margins", version: 1, sha256: "" },
      last: { at_ms: Date.parse("2026-03-02T15:00:00Z"), outcome: "met", reading: "Margin held.\nIgnored", version: 1, sha256: "", reads: 1, coalesced: false, evidence: null, scheduled_for_ms: 1, ledger: [] },
    },
  } as unknown as ScheduledTaskListItem]);
  const open = vi.fn(async () => undefined);
  render(<OpenProvider open={open}><ScriptLibrary bridge={bridge} loupistId="research" query="" onQueryChange={vi.fn()} /></OpenProvider>);
  const card = await screen.findByRole("button", { name: /Quarterly review/ });
  expect(card).toHaveTextContent("v1 · Edited by you");
  expect(card).not.toHaveTextContent("margins");
  expect(card).toHaveTextContent("Reads: Equity quote, Filings, News +1");
  expect(card).toHaveTextContent(/Met · Margin held\. · /);
  expect(card).not.toHaveTextContent("Ignored");
  await userEvent.click(screen.getByRole("button", { name: "Used by: Margin check" }));
  expect(open).toHaveBeenCalledWith({ kind: "schedule", taskId: "task" });
});

test("test submits the selected version and the arguments shown to the user", async () => {
  const bridge = new MockLoupistBridge();
  await bridge.scriptSave({ loupistId: "research", draft: {
    name: "quarters", summary: "Count quarters.", code: "print(8)",
    args: { quarters: { default: 8, description: "Number of quarters." } },
  } });
  const testRun = vi.spyOn(bridge, "scriptTest").mockResolvedValue({
    code: "print(8)", status: "completed", stdout: "8\n", stderr: "", rounds: 1, reads: [],
  });
  render(<ScriptLibrary bridge={bridge} loupistId="research" query="" onQueryChange={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: /quarters/ }));
  fireEvent.click(await screen.findByRole("button", { name: "Test" }));
  const panel = screen.getByRole("region", { name: "Test" });
  fireEvent.click(within(panel).getByRole("button", { name: "Run test" }));
  await waitFor(() => expect(testRun).toHaveBeenCalledWith({
    loupistId: "research", name: "quarters", version: 1, args: { quarters: 8 },
  }));
  expect(await within(panel).findByText("completed · 1 Python starts")).toBeInTheDocument();
});
