import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { WorkingNote } from "../../types";
import { mainTab, openApp, savedSurface } from "../renderApp";

it.each(["Chat", "Lens"])("opens an exact research note inline and restores it after visiting %s", async (destination) => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupist = (await bridge.loupistList())[0];
  const workspace = await bridge.loupistSelect(loupist.id);
  const note: WorkingNote = {
    researchId: "saved-margin-review", chatId: workspace.threads[0].id, title: "Quarterly margin review",
    markdown: "Operating margin declined in the saved quarter. Compare the filing before changing the thesis.",
    changeSummary: "Saved the current research conclusion.", recordedAtMs: Date.parse("2026-10-10T12:00:00Z"),
    previousNoteIds: [],
  };
  const search = vi.spyOn(bridge, "knowledgeResearchSearch").mockResolvedValue({
    loupistId: loupist.id,
    records: [{ id: note.researchId, chatId: note.chatId, title: note.title, recordedAt: new Date(note.recordedAtMs).toISOString(), summary: note.markdown, inputs: [] }],
  });
  const exactRead = vi.spyOn(bridge, "knowledgeWorkingNoteRead").mockResolvedValue(note);
  await openApp(bridge);
  await user.click(await mainTab(/Map/));
  await user.click(await screen.findByRole("button", { name: "Library" }));
  await user.click(await screen.findByRole("tab", { name: "Notes" }));
  await user.type(screen.getByRole("textbox", { name: "Search notes" }), "margin");
  await waitFor(() => expect(search).toHaveBeenLastCalledWith({ loupistId: loupist.id, query: "margin" }));
  await user.click(await within(screen.getByRole("list", { name: "Saved research" })).findByRole("button", { name: /Quarterly margin review/ }));
  expect(await screen.findByRole("article", { name: "Saved research note" })).toHaveTextContent(note.markdown);
  expect(exactRead).toHaveBeenCalledExactlyOnceWith({ loupistId: loupist.id, noteId: note.researchId });

  await user.click(await mainTab(new RegExp(`^${destination}$`)));
  await user.click(await mainTab(/Map/));
  if (destination === "Lens") await user.click(savedSurface("Notes"));
  await waitFor(() => expect(savedSurface("Notes")).toHaveAttribute("aria-selected", "true"));
  expect(screen.getByRole("textbox", { name: "Search notes" })).toHaveValue("margin");
  expect(await screen.findByRole("article", { name: "Saved research note" })).toHaveTextContent(note.markdown);
});
