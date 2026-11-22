import { citedFiling } from "../../test/citedFiling";
import { filingPassage } from "../../test/filingPassage";
import i18n from "../../i18n";
import { useState } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { KnowledgeLocation, RetainedSource, SourceDetail } from "../../types";
import { AddSourceDialog, SourceWorkspace } from "./SourceWorkspace";

async function fixture() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
  const source = catalog.sources[0];
  const location: KnowledgeLocation = { loupistId: catalog.loupistId, surface: "source", recordId: source.recordId, revisionId: source.revisionId, selection: { kind: "source", sourceId: source.objects[0].id } };
  return { bridge, catalog, location, onOpen: vi.fn(), onAsk: vi.fn(), onChanged: vi.fn() };
}

it("shows a readable web passage without showing the raw envelope and preserves its exact Chat reference", async () => {
  const props = await fixture();
  const originalRead = props.bridge.knowledgeSourceRead.bind(props.bridge);
  const raw = JSON.stringify({ content: "Example Domain\nReadable original passage.", content_class: "untrusted_web", charset: "utf-8" });
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async (request) => ({
    ...await originalRead(request), material: "provider_payload", selectedSpanId: "web-span",
    spans: [{ spanId: "web-span", mediaType: "application/json", selector: '{"kind":"byte_range","start":0,"end":200}', contextNote: "Passage retained from the original response.", text: raw,
      presentation: { text: "Example Domain\nReadable original passage.", sourcePointer: "/content", extractionMethod: "readability", contentOffset: 100, nextOffset: null, extractionTruncated: true } }],
  }));
  render(<SourceWorkspace {...props} />);
  const passage = await screen.findByRole("region", { name: "Saved passage" });
  expect(passage).toHaveTextContent("Readable original passage.");
  expect(screen.queryByRole("button", { name: "Show the whole saved text" })).toBeNull();
  expect(passage).not.toHaveTextContent("untrusted_web");
  const user = userEvent.setup();
  await user.click(screen.getByText("Source details"));
  expect(screen.queryByText(raw)).not.toBeInTheDocument();
  expect(screen.getByText("Passage retained from the original response.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Add to question" }));
  expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ recordId: props.location.recordId, revisionId: props.location.revisionId, spanId: "web-span" }));
});

it.each([
  ["en", "Saved data", "Showing 2 of 5 rows", "Show original record", "Hide original record", "True"],
  ["ko", "저장된 데이터", "5개 행 중 2개 표시", "원본 레코드 보기", "원본 레코드 숨기기", "참"],
])("reads saved data in %s and reveals the raw payload only on request", async (language, heading, caption, show, hide, trueLabel) => {
  await act(() => i18n.changeLanguage(language));
  try {
    const props = await fixture();
    const originalRead = props.bridge.knowledgeSourceRead.bind(props.bridge);
    const raw = '{"dataset_id":"internal-id","rows":[{"period":"2024","value":"123975371"}]}';
    const figure = { threadId: "chat", messageId: "answer", evidenceId: "population", linkIndex: 0 };
    const read = vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async request => ({
      ...await originalRead(request), sourceClass: "tool_result", selectedSpanId: "data-span",
      spans: [{ spanId: "data-span", mediaType: "application/json", selector: "", contextNote: "", text: raw,
        presentation: { text: raw, sourcePointer: "", extractionMethod: "structured_data", contentOffset: 0, nextOffset: null, extractionTruncated: false,
          table: { columns: ["Economy", "Period", "Population, total", "Reported"], rowOffset: 3, totalRows: 5, focusedCell: { row: 1, column: 2 }, rows: [
            [{ kind: "text", value: "Japan" }, { kind: "text", value: "2023" }, { kind: "number", value: "124516650" }, { kind: "empty" }],
            [{ kind: "text", value: "Japan" }, { kind: "text", value: "2024" }, { kind: "number", value: "123975371" }, { kind: "boolean", value: true }],
          ] },
        },
      }],
    }));
    render(<SourceWorkspace {...props} readerOnly sourceOpen={{ figure }} />);
    const section = await screen.findByRole("region", { name: heading });
    const table = within(section).getByRole("table", { name: caption });
    expect(within(table).getByRole("columnheader", { name: "Population, total" })).toBeVisible();
    expect(within(table).getByRole("cell", { name: "2024" })).toBeVisible();
    expect(within(table).getByRole("cell", { name: trueLabel })).toBeVisible();
    const value = within(table).getByRole("cell", { name: "123,975,371" });
    expect(value.querySelector("mark")).toHaveAttribute("aria-current", "true");
    expect(read).toHaveBeenCalledWith(expect.objectContaining({ figure }));
    expect(screen.queryByText(/internal-id/)).toBeNull();

    const user = userEvent.setup();
    await user.click(within(section).getByText(show));
    expect(await within(section).findByText(raw, { selector: "pre" })).toBeVisible();
    await user.click(within(section).getByText(hide));
    await waitFor(() => expect(screen.queryByText(/internal-id/)).toBeNull());
    expect(table).toBeVisible();
  } finally {
    await act(() => i18n.changeLanguage("en"));
  }
});

it("retries the same exact source and focuses the restored reader", async () => {
  const props = await fixture();
  const read = props.bridge.knowledgeSourceRead.bind(props.bridge);
  const calls = vi.spyOn(props.bridge, "knowledgeSourceRead").mockRejectedValueOnce(new Error("Temporarily unavailable")).mockImplementation(read);
  render(<SourceWorkspace {...props} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Temporarily unavailable");
  await userEvent.setup().click(screen.getByRole("button", { name: "Retry source" }));
  expect(await screen.findByRole("article", { name: "Source reader" })).toHaveFocus();
  expect(calls).toHaveBeenCalledTimes(2);
  expect(calls.mock.calls[1][0]).toEqual(calls.mock.calls[0][0]);
});

it("lists and searches all Loupist sources without choosing a destination", async () => {
  const props = await fixture();
  const source = props.catalog.sources[0];
  const summary = "The filing reports segment sales for the Americas and Europe. The comparative figures cover three fiscal years, including the year preceding the current reporting period.";
  const catalog = { ...props.catalog, sources: [...props.catalog.sources, { ...source, recordId: "another-source-record", title: "Another source", objects: [{ id: "another-source", title: "Unique annual filing", summary }] }] };
  const view = render(<SourceWorkspace {...props} catalog={catalog} location={{ loupistId: props.location.loupistId, surface: "source" }} />);
  expect(within(screen.getByRole("button", { name: /Unique annual filing/ })).getByText(summary)).toBeVisible();
  expect(screen.queryByRole("article", { name: "Source reader" })).toBeNull();
  view.rerender(<SourceWorkspace {...props} catalog={catalog} query="Unique annual" />);
  await userEvent.setup().click(screen.getByRole("button", { name: /Unique annual filing/ }));
  expect(props.onOpen).toHaveBeenCalledWith(expect.objectContaining({ recordId: "another-source-record", selection: { kind: "source", sourceId: "another-source" } }));
});

it("does not display a source result after changing Loupist", async () => {
  const props = await fixture();
  const original = await props.bridge.knowledgeSourceRead({ loupistId: props.location.loupistId, recordId: props.location.recordId!, revisionId: props.location.revisionId!, sourceId: props.catalog.sources[0].objects[0].id, purpose: "renderer_inspection" });
  let resolve!: (value: typeof original) => void;
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockReturnValue(new Promise((done) => { resolve = done; }));
  const view = render(<SourceWorkspace {...props} />);
  view.rerender(<SourceWorkspace {...props} catalog={{ loupistId: "another-loupist", sources: [], worlds: [], lenses: [] }} location={{ loupistId: "another-loupist", surface: "source" }} />);
  await act(async () => resolve(original));
  expect(screen.queryByRole("article", { name: "Source reader" })).toBeNull();
});

it("saves directly to the Loupist once and opens the resulting record", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  let resolve!: (value: RetainedSource) => void;
  const save = vi.spyOn(bridge, "knowledgeSourceRetain").mockReturnValue(new Promise((done) => { resolve = done; }));
  const onSaved = vi.fn();
  render(<AddSourceDialog bridge={bridge} loupistId="loupist-quality" open onOpenChange={vi.fn()} onSaved={onSaved} />);
  const user = userEvent.setup();
  await user.type(screen.getByLabelText("Source title"), "Annual filing");
  await user.click(screen.getByText("Source details"));
  await user.type(screen.getByLabelText("Source origin"), "Example issuer");
  await user.type(screen.getByLabelText("Source text"), "Capacity increased.");
  await user.dblClick(screen.getByRole("button", { name: "Save source" }));
  expect(save).toHaveBeenCalledTimes(1);
  expect(save.mock.calls[0][0]).toEqual({ loupistId: "loupist-quality", title: "Annual filing", originLocator: "Example issuer", coverage: "included_full", text: "Capacity increased.", file: undefined });
  await act(async () => resolve({ recordId: "saved-record", sourceId: "saved-source", captureId: "saved-capture", spanId: "saved-span", revisionId: "saved-revision", revisionDigest: "digest", representationId: null, selectedDigest: null, materialDigest: null }));
  expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ recordId: "saved-record", selection: { kind: "source", sourceId: "saved-source", spanId: "saved-span" } }));
});

it("retains the exact original bytes of a dropped file", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const retain = vi.spyOn(bridge, "knowledgeSourceRetain");
  render(<AddSourceDialog bridge={bridge} loupistId="loupist-quality" open onOpenChange={vi.fn()} onSaved={vi.fn()} />);
  const bytes = new Uint8Array([0, 255, 1, 2, 128]);
  const file = new File([bytes], "report.bin", { type: "application/octet-stream" });
  await userEvent.setup().click(screen.getByRole("tab", { name: "File" }));
  fireEvent.drop(screen.getByLabelText("Source file").closest("label")!, { dataTransfer: { files: [file] } });
  await waitFor(() => expect(screen.getByLabelText("Source title")).toHaveValue("report.bin"));
  await userEvent.setup().click(screen.getByRole("button", { name: "Save source" }));
  await waitFor(() => expect(retain).toHaveBeenCalledWith(expect.objectContaining({ text: "", file: { fileName: "report.bin", mediaType: "application/octet-stream", bytesBase64: "AP8BAoA=" } })));
});

it("refreshes exact source relationships and processing errors without losing the passage, focus, or scroll", async () => {
  const props = await fixture();
  const originalRead = props.bridge.knowledgeSourceRead.bind(props.bridge);
  const read = vi.spyOn(props.bridge, "knowledgeSourceRead")
    .mockImplementationOnce(async (request) => ({ ...await originalRead(request), relatedClaims: [], processing: { jobId: "pending-world", status: "queued", error: "Earlier processing issue" } }))
    .mockImplementation(async (request) => ({ ...await originalRead(request), processing: { jobId: "pending-world", status: "completed", error: null } }));
  vi.spyOn(props.bridge, "knowledgeGraphJob").mockResolvedValue({ jobId: "pending-world", loupistId: props.location.loupistId, worldRecordId: "world-protocols", worldRevisionId: "rev-owned-1", status: "completed", payloadDigest: "digest", createdAtMs: 0, hostMayRedrive: false });
  vi.useFakeTimers();
  try {
    render(<SourceWorkspace {...props} />);
    await act(async () => {});
    const reader = screen.getByRole("article", { name: "Source reader" });
    const scroller = reader.closest(".source-reader")!;
    scroller.scrollTop = 85;
    screen.getByRole("button", { name: "Add to question" }).focus();
    expect(reader).toHaveTextContent("Earlier processing issue");
    expect(screen.queryByRole("group", { name: "Relationships" })).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(2500));
    expect(screen.getByRole("status")).toHaveTextContent("World updated");
    expect(screen.getByRole("article", { name: "Source reader" })).toBe(reader);
    expect(reader).toHaveTextContent("The status-code element is a three-digit integer code.");
    expect(reader).not.toHaveTextContent("Earlier processing issue");
    expect(within(screen.getByRole("group", { name: "Relationships" })).getByText("Relationships")).toBeVisible();
    expect(screen.queryByRole("group", { name: "Facts from this Source" })).toBeNull();
    expect(screen.getByRole("button", { name: "Add to question" })).toHaveFocus();
    expect(scroller.scrollTop).toBe(85);
    expect(read).toHaveBeenCalledTimes(2);
    expect(read.mock.calls[1][0]).toEqual(read.mock.calls[0][0]);
    expect(props.onChanged).toHaveBeenCalledOnce();
  } finally { vi.useRealTimers(); }
});

it.each([
  ["en", false, "Facts from this Source", "Relationships"],
  ["ko", true, "이 출처에서 추출한 사실", "관계"],
] as const)("counts facts and distinct-name links separately in the %s Source reader", async (language, readerOnly, facts, relationships) => {
  const props = await fixture();
  const originalRead = props.bridge.knowledgeSourceRead.bind(props.bridge);
  const world = { worldRecordId: "shipment-world", worldRevisionId: "latest" };
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async (request) => ({
    ...await originalRead(request),
    relatedClaims: [
      { ...world, claimId: "january", proposition: "Ginkgo Rail reported 18 units", relation: "reports", isRelationship: false },
      { ...world, claimId: "february", proposition: "Ginkgo Rail reported 22 units", relation: "reports", isRelationship: false },
      { ...world, claimId: "supplies", proposition: "Ginkgo Rail supplies North Line", relation: "supplies", isRelationship: true },
    ],
  }));
  const previousLanguage = i18n.language;
  await i18n.changeLanguage(language);
  const view = render(<SourceWorkspace {...props} readerOnly={readerOnly} />);
  try {
    const factGroup = await screen.findByRole("group", { name: facts });
    const relationshipGroup = screen.getByRole("group", { name: relationships });
    expect(factGroup.querySelector("summary span")).toHaveTextContent("2");
    expect(relationshipGroup.querySelector("summary span")).toHaveTextContent("1");
    const user = userEvent.setup();
    await user.click(within(factGroup).getByText(facts));
    expect(within(factGroup).getAllByRole("listitem")).toHaveLength(2);
    await user.click(within(relationshipGroup).getByText(relationships));
    expect(within(relationshipGroup).getAllByRole("listitem")).toHaveLength(1);
    await user.click(within(relationshipGroup).getByRole("button", { name: /Ginkgo Rail supplies North Line/u }));
    expect(props.onOpen).toHaveBeenCalledWith(expect.objectContaining({
      surface: "world", recordId: world.worldRecordId, revisionId: world.worldRevisionId, selection: { kind: "claim", id: "supplies" },
    }));
  } finally {
    view.unmount();
    await i18n.changeLanguage(previousLanguage);
  }
});

it("reuses the exact Reader in a side panel and passes its exact passage to Chat", async () => {
  const props = await fixture();
  render(<SourceWorkspace {...props} readerOnly />);
  const reader = await screen.findByRole("article", { name: "Source reader" });
  expect(reader).toHaveTextContent("The status-code element is a three-digit integer code.");
  expect(screen.queryByRole("tablist", { name: "Source processing filters" })).toBeNull();
  expect(screen.queryByRole("list", { name: "Sources" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Close source" })).toBeNull();
  await userEvent.setup().click(screen.getByRole("button", { name: "Add to question" }));
  expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ recordId: props.location.recordId, revisionId: props.location.revisionId, spanId: expect.any(String) }));
});

it("filters source processing independently of successful source storage", async () => {
  const props = await fixture();
  const catalog = { ...props.catalog, sources: ["queued", "completed", "no_text", "failed_before_mutation"].map((status, index) => ({ ...props.catalog.sources[0], recordId: `source-${index}`, objects: [{ id: `source-${index}`, title: `Report ${index}`, processing: { jobId: null, status, error: null } }] })) };
  function Harness() {
    const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: props.location.loupistId, surface: "source" });
    return <SourceWorkspace {...props} catalog={catalog} location={location} onPatch={setLocation} />;
  }
  render(<Harness />);
  const user = userEvent.setup();
  expect(screen.getAllByRole("button", { name: /Report \d/ })).toHaveLength(4);
  await user.click(screen.getByRole("radio", { name: /Processing/ }));
  expect(screen.getAllByRole("button", { name: /Report \d/ })).toHaveLength(1);
  expect(screen.getByRole("button", { name: /Report 0/ })).toHaveTextContent("World is updating");
  await user.click(screen.getByRole("radio", { name: "Needs attention" }));
  expect(screen.getAllByRole("button", { name: /Report \d/ })).toHaveLength(2);
  expect(screen.getByRole("button", { name: /Report 2/ })).toHaveTextContent("No text for World building");
  await user.click(screen.getByRole("radio", { name: "All" }));
  expect(screen.getAllByRole("button", { name: /Report \d/ })).toHaveLength(4);
});

it("leaves the default Source saved status off a library row and shows any other status", async () => {
  const props = await fixture();
  const statuses = [undefined, "completed", "queued"];
  const catalog = { ...props.catalog, sources: statuses.map((status, index) => ({ ...props.catalog.sources[0], recordId: `source-${index}`, objects: [{ id: `source-${index}`, title: `Report ${index}`, processing: status ? { jobId: null, status, error: null } : undefined }] })) };
  render(<SourceWorkspace {...props} catalog={catalog} location={{ loupistId: props.location.loupistId, surface: "source" }} />);
  const rows = await screen.findAllByRole("button", { name: /Report \d/ });
  expect(rows).toHaveLength(3);
  for (const row of rows) expect(row).not.toHaveTextContent("Source saved");
  expect(screen.getByRole("button", { name: /Report 2/ })).toHaveTextContent("World is updating");
});

it("says World building is paused only for the user's pause, and names a source that stopped by itself", async () => {
  const props = await fixture();
  const catalog = { ...props.catalog, sources: ["paused", "paused_budget"].map((status, index) => ({ ...props.catalog.sources[0], recordId: `source-${index}`, objects: [{ id: `source-${index}`, title: `Report ${index}`, processing: { jobId: "job", status, error: null } }] })) };
  render(<SourceWorkspace {...props} catalog={catalog} location={{ loupistId: props.location.loupistId, surface: "source" }} />);
  expect(await screen.findByRole("button", { name: /Report 0/ })).toHaveTextContent("World building paused");
  const stopped = screen.getByRole("button", { name: /Report 1/ });
  expect(stopped).toHaveTextContent("World building stopped for this source");
  expect(stopped).not.toHaveTextContent("World building paused");
});

it("opens an exact World relationship from its original source", async () => {
  const props = await fixture();
  render(<SourceWorkspace {...props} />);
  const relationships = await screen.findByRole("group", { name: "Relationships" });
  await userEvent.setup().click(within(relationships).getByText("Relationships"));
  const claim = relationships.querySelector("button")!;
  expect(claim).toBeTruthy();
  await userEvent.setup().click(claim);
  expect(props.onOpen).toHaveBeenCalledWith(expect.objectContaining({ surface: "world", recordId: "world-protocols", revisionId: "rev-owned-1", selection: { kind: "claim", id: expect.any(String) } }));
});

it("preserves a source list's scroll and focus when its Reader closes", async () => {
  const props = await fixture();
  function Harness() {
    const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: props.location.loupistId, surface: "source", view: { search: "RFC", sourceScrollTop: 95 } });
    return <SourceWorkspace {...props} location={location} onPatch={setLocation} onOpen={setLocation} />;
  }
  const view = render(<Harness />);
  const library = view.container.querySelector(".source-library")!;
  expect(library.scrollTop).toBe(95);
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: /RFC 9110 HTTP semantics/ }));
  await user.click(await screen.findByRole("button", { name: "Close source" }));
  expect(library.scrollTop).toBe(95);
  expect(screen.getByRole("button", { name: /RFC 9110 HTTP semantics/ })).toHaveFocus();
});

it("exports the selected original binary file through the authorized source request", async () => {
  const props = await fixture();
  const originalRead = props.bridge.knowledgeSourceRead.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async (request) => ({ ...await originalRead(request), material: "original", spans: [{ spanId: "binary-span", mediaType: "application/pdf", selector: "whole_representation", contextNote: "Original file bytes", text: null }], selectedSpanId: "binary-span" }));
  const exportFile = vi.spyOn(props.bridge, "knowledgeSourceExport").mockResolvedValue("/tmp/report.pdf");
  render(<SourceWorkspace {...props} />);
  await userEvent.setup().click(await screen.findByRole("button", { name: "Export original file" }));
  expect(exportFile).toHaveBeenCalledWith(expect.objectContaining({ loupistId: props.location.loupistId, recordId: props.location.recordId, revisionId: props.location.revisionId, spanId: "binary-span", purpose: "renderer_inspection" }));
  expect(await screen.findByText("File exported.")).toBeVisible();
});

it("opens the original source in the supplied workspace reader without losing its exact saved citation", async () => {
  const props = await fixture();
  const read = props.bridge.knowledgeSourceRead.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async (request) => ({ ...await read(request), originLabel: "https://issuer.example/filings/2025-q3.pdf" }));
  const onOpenLink = vi.fn();
  const external = vi.spyOn(props.bridge, "openExternalUrl");
  render(<SourceWorkspace {...props} onOpenLink={onOpenLink} opensLinksInPanel readerOnly />);
  const original = await screen.findByRole("button", { name: /Open original/ });
  expect(original.querySelector(".lucide-external-link")).not.toBeInTheDocument();
  await userEvent.setup().click(original);
  await waitFor(() => expect(onOpenLink).toHaveBeenCalledWith("https://issuer.example/filings/2025-q3.pdf"));
  expect(external).not.toHaveBeenCalled();
  await userEvent.setup().click(screen.getByRole("button", { name: "Add to question" }));
  expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ recordId: props.location.recordId, revisionId: props.location.revisionId, spanId: expect.any(String) }));
});

it("shows pasted material as user-provided in the Library and the reader, with what the user stated", async () => {
  const props = await fixture();
  const record = props.catalog.sources[0];
  const catalog = { ...props.catalog, sources: [{ ...record, objects: [{ ...record.objects[0], title: "Solsup backlog", userProvided: true, originLabel: "Pasted text" }] }] };
  const view = render(<SourceWorkspace {...props} catalog={catalog} location={{ loupistId: props.location.loupistId, surface: "source" }} />);
  const row = screen.getByRole("button", { name: /Solsup backlog/ });
  expect(row).toHaveTextContent("User-provided");
  expect(row).toHaveTextContent("Pasted text");
  const read = props.bridge.knowledgeSourceRead.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async (request) => ({
    ...await read(request), sourceClass: "user_provided", originLabel: "https://fictional.example/solsup/backlog-2025",
    provided: { chatId: "chat-a", messageId: "user-paste", pastedAtMs: Date.UTC(2026, 10, 19, 12), statedPublisher: "솔숲식품", statedUrl: "https://fictional.example/solsup/backlog-2025" },
  }));
  view.rerender(<SourceWorkspace {...props} catalog={catalog} />);
  const reader = await screen.findByRole("article", { name: "Source reader" });
  expect(within(reader).getByText("User-provided")).toBeVisible();
  expect(within(reader).getByRole("button", { name: /Link you gave \(fictional\.example\)/ })).toBeVisible();
  const note = within(reader).getByTestId("source-provided");
  expect(note).toHaveTextContent("Pasted into a Chat on Nov 19, 2026");
  expect(note).toHaveTextContent("publisher as you stated it: 솔숲식품");
  expect(note).toHaveTextContent("were not verified");
  try {
    await act(async () => { await i18n.changeLanguage("ko"); });
    view.rerender(<SourceWorkspace {...props} catalog={catalog} />);
    expect(await within(screen.getByRole("article")).findByText("사용자 제공")).toBeVisible();
    expect(screen.getByTestId("source-provided")).toHaveTextContent("알려 주신 발행처: 솔숲식품");
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});

it("keeps a source locator with an unsupported scheme as plain text", async () => {
  const props = await fixture();
  const read = props.bridge.knowledgeSourceRead.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async (request) => ({ ...await read(request), originLabel: "javascript:alert(1)" }));
  render(<SourceWorkspace {...props} />);
  expect(await screen.findByText("javascript:alert(1)")).toBeVisible();
  expect(screen.queryByRole("button", { name: /Open original/ })).toBeNull();
});

it("shows a publication day without shifting its calendar date", async () => {
  const [locale, expected] = ["ko", "2026년 10월 27일"];
  await i18n.changeLanguage(locale);
  try {
    const props = await fixture();
    const record = props.catalog.sources[0];
    const catalog = { ...props.catalog, sources: [{ ...record, objects: [{
      ...record.objects[0], title: "Calendar date source", publicationTime: { precision: "date" as const, value: "2026-10-27" },
    }] }] };
    render(<SourceWorkspace {...props} catalog={catalog} location={{ loupistId: props.location.loupistId, surface: "source" }} />);
    const row = screen.getByRole("button", { name: /Calendar date source/ });
    expect(within(row).getByRole("time")).toHaveTextContent(expected);
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});

it("shows the What you follow Source in the interface language whatever title it was stored with", async () => {
  const props = await fixture();
  const record = props.catalog.sources[0];
  const followed = { ...record, title: "What you follow", titleKey: "followed" as const, objects: [{
    // An older host stored this origin label; the interface's own wording replaces it.
    ...record.objects[0], title: "What you follow", titleKey: "followed" as const, originLabel: "What you follow and your holdings",
  }] };
  const catalog = { ...props.catalog, sources: [followed] };
  const location = { loupistId: props.location.loupistId, surface: "source" as const };
  const view = render(<SourceWorkspace {...props} catalog={catalog} location={location} />);
  const english = screen.getByRole("button", { name: /What you follow.*What you follow/ });
  expect(english).toBeVisible();
  expect(english).not.toHaveTextContent("holdings");
  try {
    await act(async () => { await i18n.changeLanguage("ko"); });
    view.rerender(<SourceWorkspace {...props} catalog={catalog} location={location} />);
    const row = screen.getByRole("button", { name: /관심 자산/ });
    expect(row).toHaveTextContent("관심 자산");
    expect(row).not.toHaveTextContent("보유");
    expect(row).not.toHaveTextContent("What you follow");
    // The title the user reads is the one they can search for.
    view.rerender(<SourceWorkspace {...props} catalog={catalog} location={location} query="관심" />);
    expect(screen.getByRole("button", { name: /관심 자산/ })).toBeVisible();
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});

describe("Try again for a Source whose World building stopped", () => {
  const stoppedCatalog = (props: Awaited<ReturnType<typeof fixture>>, statuses: string[]) => ({ ...props.catalog, sources: statuses.map((status, index) => ({ ...props.catalog.sources[0], recordId: `source-${index}`, objects: [{ id: `source-${index}`, title: `Report ${index}`, processing: { jobId: `job-${index}`, status, error: null } }] })) });
  const graphJob = (status: string) => ({ jobId: "job-1", loupistId: "loupist-quality", worldRecordId: "world-protocols", worldRevisionId: "rev-owned-1", status, payloadDigest: "digest", createdAtMs: 0, hostMayRedrive: false });

  it("offers the action only on a row that stopped by itself and explains the allowance it uses", async () => {
    const props = await fixture();
    render(<SourceWorkspace {...props} catalog={stoppedCatalog(props, ["paused", "paused_budget", "failed_before_mutation", "queued"])} location={{ loupistId: props.location.loupistId, surface: "source" }} />);
    await screen.findByRole("button", { name: /Report 0/ });
    expect(screen.getAllByRole("button", { name: "Try again" })).toHaveLength(1);
    const item = screen.getByRole("button", { name: /Report 1/ }).closest("li")!;
    expect(within(item).getByRole("button", { name: "Try again" })).toBeVisible();
    expect(item).toHaveTextContent("Uses one more allowance of model requests per source, as set in World building.");
  });

  it("disables while the host decides, shows its refusal inline, and lets the user try again", async () => {
    const props = await fixture();
    let refuse!: (cause: Error) => void;
    const retry = vi.spyOn(props.bridge, "knowledgeGraphRetrySource").mockReturnValueOnce(new Promise((_, reject) => { refuse = reject; }));
    render(<SourceWorkspace {...props} catalog={stoppedCatalog(props, ["paused", "paused_budget"])} location={{ loupistId: props.location.loupistId, surface: "source" }} />);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Try again" }));
    expect(retry).toHaveBeenCalledWith({ loupistId: props.location.loupistId, recordId: "source-1" });
    expect(screen.getByRole("button", { name: "Trying again…" })).toBeDisabled();
    await act(async () => refuse(new Error("enable World building with a sufficient allowance before retrying")));
    expect(screen.getByRole("alert")).toHaveTextContent("enable World building with a sufficient allowance before retrying");
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
    expect(props.onChanged).not.toHaveBeenCalled();
  });

  it("refreshes the library after the host accepts and follows the returned job until it settles", async () => {
    const props = await fixture();
    vi.spyOn(props.bridge, "knowledgeGraphRetrySource").mockResolvedValue(graphJob("queued"));
    const read = vi.spyOn(props.bridge, "knowledgeGraphJob").mockResolvedValueOnce(graphJob("running")).mockResolvedValue(graphJob("completed"));
    vi.useFakeTimers();
    try {
      render(<SourceWorkspace {...props} catalog={stoppedCatalog(props, ["paused_budget"])} location={{ loupistId: props.location.loupistId, surface: "source" }} />);
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Try again" })); });
      expect(props.onChanged).toHaveBeenCalledOnce();
      await act(async () => vi.advanceTimersByTimeAsync(2500));
      expect(read).toHaveBeenCalledWith({ loupistId: props.location.loupistId, jobId: "job-1" });
      expect(props.onChanged).toHaveBeenCalledOnce();
      await act(async () => vi.advanceTimersByTimeAsync(2500));
      expect(props.onChanged).toHaveBeenCalledTimes(2);
      await act(async () => vi.advanceTimersByTimeAsync(10_000));
      expect(read).toHaveBeenCalledTimes(2);
    } finally { vi.useRealTimers(); }
  });

  it("offers the action in the reader and refreshes it to World building progress", async () => {
    const props = await fixture();
    const originalRead = props.bridge.knowledgeSourceRead.bind(props.bridge);
    vi.spyOn(props.bridge, "knowledgeSourceRead")
      .mockImplementationOnce(async (request) => ({ ...await originalRead(request), processing: { jobId: "job-1", status: "paused_budget", error: null } }))
      .mockImplementation(async (request) => ({ ...await originalRead(request), processing: { jobId: "job-1", status: "queued", error: null } }));
    const retry = vi.spyOn(props.bridge, "knowledgeGraphRetrySource").mockResolvedValue(graphJob("queued"));
    render(<SourceWorkspace {...props} readerOnly />);
    const reader = await screen.findByRole("article", { name: "Source reader" });
    expect(within(reader).getByRole("status")).toHaveTextContent("World building stopped for this source");
    await userEvent.setup().click(within(reader).getByRole("button", { name: "Try again" }));
    expect(retry).toHaveBeenCalledWith({ loupistId: props.location.loupistId, recordId: props.location.recordId });
    expect(await within(reader).findByText("World is updating")).toBeVisible();
    expect(within(reader).queryByRole("button", { name: "Try again" })).toBeNull();
    expect(props.onChanged).toHaveBeenCalled();
  });

  it("shows no action in the reader for any other status", async () => {
    const props = await fixture();
    const originalRead = props.bridge.knowledgeSourceRead.bind(props.bridge);
    vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async (request) => ({ ...await originalRead(request), processing: { jobId: "job-1", status: "paused", error: null } }));
    render(<SourceWorkspace {...props} readerOnly />);
    await screen.findByRole("article", { name: "Source reader" });
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  });
});

it("shows neutral metadata for source details saved by a newer app", async () => {
  const props = await fixture();
  const read = props.bridge.knowledgeSourceRead.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async request => ({
    ...await read(request),
    coverage: "future_coverage" as SourceDetail["coverage"],
    material: "future_material" as SourceDetail["material"],
    sourceClass: "future_class" as SourceDetail["sourceClass"],
  }));
  render(<SourceWorkspace {...props} />);
  await userEvent.setup().click(await screen.findByText("Source details"));
  expect(screen.getAllByText("Unspecified", { selector: "dd" })).toHaveLength(2);
  expect(screen.getByText("Unspecified", { selector: ".memory-meta-row > [data-slot=badge]" })).toBeVisible();
});

it.each([["web_fetch", "Read a web source"], ["unknown_custom_read", "Source saved"]])("names the %s activity in library metadata", async (tool, label) => {
  const props = await fixture();
  const read = props.bridge.knowledgeSourceRead.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async request => ({
    ...await read(request), sourceClass: "tool_result", originLabel: `tool:${tool}`,
  }));
  render(<SourceWorkspace {...props} />);
  await waitFor(() => expect(document.querySelector(".memory-meta-row")).toHaveTextContent(label));
  expect(document.querySelector(".memory-meta-row")).not.toHaveTextContent(/Tool result|tool:/);
});

it("renders saved filing rows as a table while retaining the exact selection offsets", async () => {
  const props = await fixture();
  const read = props.bridge.knowledgeSourceRead.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async request => ({
    ...await read(request), spans: [{ spanId: "filing", mediaType: "text/plain", selector: "lines:1-5", contextNote: "", text: filingPassage }],
  }));
  render(<SourceWorkspace {...props} readerOnly />);
  const amount = await screen.findByRole("cell", { name: "$162,560" });
  const passage = screen.getByTestId("knowledge-passage");
  expect(passage.textContent).toBe(filingPassage);
  const range = document.createRange();
  range.selectNodeContents(passage);
  const amountText = Array.from(amount.childNodes).find(node => node.nodeType === Node.TEXT_NODE)!;
  range.setEnd(amountText, amountText.textContent!.indexOf("$"));
  expect(range.toString().length).toBe(filingPassage.indexOf("$162,560"));
  expect(screen.getByRole("columnheader", { name: "2023" })).toBeVisible();
});

async function citedFilingFixture() {
  const props = await fixture();
  const read = props.bridge.knowledgeSourceRead.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeSourceRead").mockImplementation(async request => ({
    ...await read(request.spanId === citedFiling.spanId ? { ...request, spanId: undefined } : request),
    material: "provider_payload", selectedSpanId: citedFiling.spanId, spans: [citedFiling],
  }));
  return props;
}

it("opens the cited table with its headers and figures, then steps to a distant passage", async () => {
  const props = await citedFilingFixture();
  const user = userEvent.setup();
  const scrolled: number[] = [];
  const originalRect = HTMLElement.prototype.getBoundingClientRect;
  const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    if (this.dataset.citedPassage !== undefined) {
      const index = Number(this.dataset.citedPassage);
      scrolled.push(index);
      return new DOMRect(0, (index + 1) * 100, 400, 80);
    }
    return originalRect.call(this);
  });
  try {
    render(<SourceWorkspace {...props} readerOnly />);
    const passages = await screen.findAllByTestId("knowledge-passage");
    expect(passages).toHaveLength(2);
    expect(passages[0]).not.toHaveTextContent("Clipped policy text");
    expect(within(passages[0]).getByRole("columnheader", { name: "2024" })).toBeVisible();
    expect([...passages[0].querySelectorAll("mark.evidence-match")].map(mark => mark.textContent)).toEqual(["96,169", "78,129", "391,035", "394,328"]);
    expect(passages[1].querySelector("mark.evidence-match")).toHaveTextContent("$700");
    expect(screen.getByRole("img", { name: "Text between the cited passages is not shown here" })).toHaveTextContent("…");
    const previous = screen.getByRole("button", { name: "Previous cited passage" });
    const next = screen.getByRole("button", { name: "Next cited passage" });
    expect(previous).toBeDisabled();
    expect(screen.getByText("1 of 2")).toBeVisible();
    expect(scrolled).toEqual([0]);
    await user.click(next);
    expect(next).toBeDisabled();
    expect(screen.getByText("2 of 2")).toBeVisible();
    expect(scrolled).toEqual([0, 1]);
    await user.click(previous);
    expect(scrolled).toEqual([0, 1, 0]);
    await user.click(screen.getByRole("button", { name: "Show the whole saved text" }));
    expect(screen.getByTestId("knowledge-passage").textContent).toBe(citedFiling.presentation!.text);
    await user.click(screen.getByRole("button", { name: "Next cited passage" }));
    expect(screen.getAllByTestId("knowledge-passage")).toHaveLength(2);
    expect(screen.getByText("2 of 2")).toBeVisible();
  } finally { rect.mockRestore(); }
});

it("adds a library selection using offsets into the complete saved text", async () => {
  const props = await citedFilingFixture();
  const resolve = vi.spyOn(props.bridge, "knowledgeSourceSelectionResolve");
  render(<SourceWorkspace {...props} readerOnly quoteCitedSelections />);
  const [, second] = await screen.findAllByTestId("knowledge-passage");
  const range = document.createRange();
  range.selectNodeContents(second!);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  fireEvent.mouseUp(second!);
  const action = await waitFor(() => {
    const button = document.querySelector<HTMLButtonElement>(".quote-ask");
    expect(button).toHaveTextContent("Add to question");
    return button!;
  });
  fireEvent.click(action);
  const text = "Restricted cash was $700 million.";
  const startUtf16 = citedFiling.presentation!.text.indexOf(text);
  const offsets = { startUtf16, endUtf16: startUtf16 + text.length, text };
  expect(resolve).toHaveBeenCalledWith({ source: expect.objectContaining({ spanId: citedFiling.spanId }), selection: offsets });
  await waitFor(() => expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ spanId: citedFiling.spanId, selection: offsets })));
});

it("opens the first cited passage quietly when the host cannot focus a requested figure", async () => {
  const props = await citedFilingFixture();
  const read = vi.mocked(props.bridge.knowledgeSourceRead);
  const fixtureRead = read.getMockImplementation()!;
  read.mockImplementation(async request => ({ ...await fixtureRead(request), figureFailure: "mark_unavailable" }));
  const figure = { threadId: "chat", messageId: "answer", evidenceId: "citation", linkIndex: 1 };
  render(<SourceWorkspace {...props} readerOnly sourceOpen={{ figure }} />);

  expect(await screen.findByText("1 of 2")).toBeVisible();
  expect(screen.getByRole("button", { name: "Previous cited passage" })).toBeDisabled();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(read).toHaveBeenCalledWith(expect.objectContaining({ figure }));
});
