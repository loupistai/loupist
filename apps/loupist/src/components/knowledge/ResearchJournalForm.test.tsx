import { fireEvent, render, screen } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { ResearchJournalForm } from "./ResearchJournalForm";

it("creates a user's judgment without a check or confidence", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const create = vi.spyOn(bridge, "journalCreate");
  const onSaved = vi.fn();
  render(<ResearchJournalForm bridge={bridge} loupistId="loupist-quality" onSaved={onSaved} onCancel={vi.fn()} />);
  fireEvent.change(screen.getByRole("textbox", { name: "What do you think?" }), { target: { value: "Margins could recover" } });
  fireEvent.click(screen.getByRole("button", { name: "Save entry" }));
  await vi.waitFor(() => expect(onSaved).toHaveBeenCalledOnce());
  expect(create).toHaveBeenCalledWith({ loupistId: "loupist-quality", judgment: "Margins could recover", reasoning: "", check: null, entities: [] });
});

it("saves an optional check without inferring confidence and edits the same entry", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const entry = await bridge.journalCreate({ loupistId: "loupist-quality", judgment: "Margins could recover", check: { claim: "Gross margin exceeds 55%", metric: "Gross margin", direction: "above", threshold: 55, unit: "%", byDate: "2027-03-31" } });
  const update = vi.spyOn(bridge, "journalUpdate");
  const onSaved = vi.fn();
  render(<ResearchJournalForm bridge={bridge} loupistId="loupist-quality" entry={entry} onSaved={onSaved} onCancel={vi.fn()} />);
  expect(screen.getByRole("combobox", { name: "Confidence (optional)" })).toHaveValue("");
  fireEvent.change(screen.getByRole("textbox", { name: "Why? (optional)" }), { target: { value: "Costs are easing" } });
  fireEvent.click(screen.getByRole("button", { name: "Save entry" }));
  await vi.waitFor(() => expect(onSaved).toHaveBeenCalledOnce());
  expect(update).toHaveBeenCalledWith(expect.objectContaining({ journalId: entry.journalId, expectedUpdatedAtMs: entry.updatedAtMs, reasoning: "Costs are easing", check: entry.check }));
  expect((await bridge.knowledgeJournalRead({ loupistId: "loupist-quality", journalId: entry.journalId })).check?.confidence).toBeUndefined();
});
