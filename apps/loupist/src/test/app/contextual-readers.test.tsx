import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mainTab, openApp } from "../renderApp";

it("returns from a note's exact evidence to the note and continues its original Chat", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = "loupist-quality";
  const workspace = await bridge.loupistSelect(loupistId);
  const thread = workspace.threads.find((item) => item.title === "How should we read the margin decline?")!;
  const catalog = await bridge.knowledgeCatalog({ loupistId });
  const lens = catalog.lenses.find((item) => item.lensKind === "perspective") ?? catalog.lenses[0];
  const source = catalog.sources[0];
  const detail = await bridge.knowledgeSourceRead({ loupistId, recordId: source.recordId, revisionId: source.revisionId, sourceId: source.objects[0].id, purpose: "renderer_inspection" });
  vi.spyOn(bridge, "knowledgeLensChanges").mockResolvedValue([{
    changeId: "note-backed-change", recordId: lens.recordId, newRevisionId: lens.revisionId,
    rationale: "A saved note changed the method.",
    inputs: { references: [{ kind: "research", recordId: "", revisionId: "", objectId: "saved-note" }] },
  }]);
  vi.spyOn(bridge, "knowledgeWorkingNoteRead").mockResolvedValue({
    researchId: "saved-note", chatId: thread.id, title: "The original note", markdown: "The conclusion remained conditional.",
    recordedAtMs: 1, changeSummary: "Saved evidence", previousNoteIds: [],
    inputs: { sources: [{ receiptId: "note-source", recordId: source.recordId, revisionId: source.revisionId, spanId: detail.spans[0].spanId, coverage: "included_full", attribution: detail.title }], worlds: [], lenses: [] },
  });
  const search = vi.spyOn(bridge, "knowledgeResearchSearch");
  await openApp(bridge);
  await user.click(await mainTab(/^Lens$/));
  await user.click(await screen.findByRole("tab", { name:/Ways of working/ }));
    await user.click(await screen.findByRole("button", { name: new RegExp(lens.title) }));
  await user.click(await screen.findByText("Saved basis"));
  await user.click(await screen.findByRole("button", { name: "Working note 1" }));
  // A note opens over the Lens tab and carries its own evidence trail.
  const note = await screen.findByRole("dialog", { name: "Saved note" });
  expect(await within(note).findByRole("heading", { name: "The original note" })).toBeVisible();
  expect(search).not.toHaveBeenCalled();
  await user.click(within(note).getByRole("button", { name: "Open source 1" }));
  const sourceReader = await screen.findByRole("article", { name: "Source reader" });
  expect(await within(sourceReader).findByRole("heading", { name: detail.title })).toBeVisible();
  await user.click(screen.getByRole("button", { name: /^Back$/ }));
  const restored = await screen.findByRole("dialog", { name: "Saved note" });
  expect(await within(restored).findByText("The conclusion remained conditional.")).toBeVisible();
  await user.click(within(restored).getByRole("button", { name: "Continue chat" }));
  expect(await screen.findByRole("heading", { name: thread.title })).toBeVisible();
  expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveFocus();
  expect(screen.queryByRole("complementary", { name: "Document inspector" })).not.toBeInTheDocument();
});
