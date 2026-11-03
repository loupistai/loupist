import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { KnowledgeLocation, ResearchRecord } from "../../types";
import { ResearchLibrary } from "./ResearchLibrary";

const record: ResearchRecord = { id: "saved-note", title: "Scope comparison", recordedAt: "2026-10-14T09:00:00Z", inputs: [] };
function Harness({ bridge }: { bridge: MockLoupistBridge }) {
  const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: "loupist-quality", surface: "note" });
  return <ResearchLibrary bridge={bridge} location={location} query={location.view?.search ?? ""} onQueryChange={(search) => setLocation({ ...location, view: { ...location.view, search } })} onPatch={setLocation} onOpen={setLocation} onAsk={vi.fn()} />;
}

it("keeps the research selection while filtering and returns keyboard focus when closing it", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const search = vi.spyOn(bridge, "knowledgeResearchSearch").mockImplementation(async ({ loupistId, query }) => ({ loupistId, records: query === "missing" ? [] : [record] }));
  vi.spyOn(bridge, "knowledgeWorkingNoteRead").mockResolvedValue({ researchId: record.id, chatId: "chat", title: record.title, markdown: "Saved comparison body", changeSummary: "", recordedAtMs: 1 });
  render(<Harness bridge={bridge} />);
  fireEvent.click(await screen.findByRole("button", { name: /Scope comparison/ }));
  await screen.findByRole("article", { name: "Saved working note" });
  expect(screen.getByRole("button", { name: /Scope comparison/ })).toHaveAttribute("aria-current", "true");
  fireEvent.change(screen.getByRole("textbox", { name: "Search notes" }), { target: { value: "missing" } });
  await screen.findByText("No matching research");
  expect(screen.getByText("Saved comparison body")).toBeVisible();
  expect(search).toHaveBeenLastCalledWith({ loupistId: "loupist-quality", query: "missing" });
  fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
  await screen.findByRole("button", { name: /Scope comparison/ });
  fireEvent.click(screen.getByRole("button", { name: "Close record" }));
  await waitFor(() => expect(screen.getByRole("button", { name: /Scope comparison/ })).toHaveFocus());
});

it("retries a failed research search and refuses results from another Loupist", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const search = vi.spyOn(bridge, "knowledgeResearchSearch").mockResolvedValueOnce({ loupistId: "another-loupist", records: [record] }).mockResolvedValue({ loupistId: "loupist-quality", records: [record] });
  render(<Harness bridge={bridge} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("requested Loupist");
  expect(screen.queryByText(record.title)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Retry research" }));
  await screen.findByRole("button", { name: /Scope comparison/ });
  expect(search).toHaveBeenCalledTimes(2);
});

const note: ResearchRecord = { id: "note-exact", title: "Issuer margin review", recordedAt: "2026-10-10T12:00:00Z", summary: "Operating margin increased.", inputs: [] };
const journal: ResearchRecord = { id: "journal-exact", title: "Keep the original thesis", recordedAt: "2026-10-09T12:00:00Z", journal: { sectionId: "journal-exact", conclusion: "Keep the original thesis" }, inputs: [] };
function setup() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const search = vi.spyOn(bridge, "knowledgeResearchSearch").mockResolvedValue({ loupistId: "loupist-quality", records: [note, journal] });
  vi.spyOn(bridge, "journalList").mockResolvedValue([{ loupistId: "loupist-quality", journalId: "journal-exact", researchId: "", judgment: "Keep the original thesis", reasoning: "", entities: [], createdAtMs: 1, updatedAtMs: 1, archived: false, inputDigest: "fixture", inputs: { sources: [], worlds: [], lenses: [] } }]);
  const props = { bridge, location: { loupistId: "loupist-quality", surface: "note" as const }, query: "", onQueryChange: vi.fn(), onOpen: vi.fn(), onPatch: vi.fn(), onAsk: vi.fn() };
  return { props, search };
}

it("finds saved notes and opens notes and journals by their exact stored identifiers", async () => {
  const { props, search } = setup();
  render(<ResearchLibrary {...props} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: /Issuer margin review/ }));
  expect(search).toHaveBeenCalledWith({ loupistId: "loupist-quality", query: undefined });
  expect(props.onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ surface: "note", objectId: "note-exact", selection: { kind: "research", recordId: "note-exact" } }));
  await user.click(screen.getByRole("button", { name: /Keep the original thesis/ }));
  expect(props.onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ surface: "note", objectId: "journal-exact", selection: { kind: "journal", journalId: "journal-exact" } }));
});

it("reads a selected saved note and keeps its web links inside the supplied reader", async () => {
  const { props } = setup();
  const read = vi.spyOn(props.bridge, "knowledgeWorkingNoteRead").mockResolvedValue({ researchId: note.id, chatId: "chat-original", title: note.title, markdown: "Read [the filing](https://issuer.example/filing).", changeSummary: "", recordedAtMs: 123 });
  const onOpenLink = vi.fn();
  render(<ResearchLibrary {...props} onOpenLink={onOpenLink} location={{ ...props.location, selection: { kind: "research", recordId: note.id } }} />);
  await userEvent.setup().click(await screen.findByRole("link", { name: "the filing" }));
  expect(read).toHaveBeenCalledWith({ loupistId: "loupist-quality", noteId: note.id });
  expect(onOpenLink).toHaveBeenCalledWith("https://issuer.example/filing");
  await userEvent.setup().click(screen.getByRole("button", { name: "Continue chat" }));
  expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ objectId: note.id }), "chat-original");
});
