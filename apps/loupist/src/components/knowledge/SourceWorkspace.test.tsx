import i18n from "../../i18n";
import { useState } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { KnowledgeLocation, RetainedSource } from "../../types";
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
  expect(passage).not.toHaveTextContent("untrusted_web");
  const user = userEvent.setup();
  await user.click(screen.getByText("Source details"));
  expect(screen.queryByText(raw)).not.toBeInTheDocument();
  expect(screen.getByText("Passage retained from the original response.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Add to question" }));
  expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ recordId: props.location.recordId, revisionId: props.location.revisionId, spanId: "web-span" }));
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
  const catalog = { ...props.catalog, sources: [...props.catalog.sources, { ...source, recordId: "another-source-record", title: "Another source", objects: [{ id: "another-source", title: "Unique annual filing" }] }] };
  const view = render(<SourceWorkspace {...props} catalog={catalog} location={{ loupistId: props.location.loupistId, surface: "source" }} />);
  expect(screen.getByRole("button", { name: /Unique annual filing/ })).toBeVisible();
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
    expect(screen.queryByRole("group", { name: "Related relationships" })).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(2500));
    expect(screen.getByRole("status")).toHaveTextContent("World updated");
    expect(screen.getByRole("article", { name: "Source reader" })).toBe(reader);
    expect(reader).toHaveTextContent("The status-code element is a three-digit integer code.");
    expect(reader).not.toHaveTextContent("Earlier processing issue");
    expect(within(screen.getByRole("group", { name: "Related relationships" })).getByText(/Relationships using this source/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Add to question" })).toHaveFocus();
    expect(scroller.scrollTop).toBe(85);
    expect(read).toHaveBeenCalledTimes(2);
    expect(read.mock.calls[1][0]).toEqual(read.mock.calls[0][0]);
    expect(props.onChanged).toHaveBeenCalledOnce();
  } finally { vi.useRealTimers(); }
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
  expect(screen.getByRole("button", { name: /Report 2/ })).toHaveTextContent("No text for World processing");
  await user.click(screen.getByRole("radio", { name: "All" }));
  expect(screen.getAllByRole("button", { name: /Report \d/ })).toHaveLength(4);
});

it("opens an exact World relationship from its original source", async () => {
  const props = await fixture();
  render(<SourceWorkspace {...props} />);
  const relationships = await screen.findByRole("group", { name: "Related relationships" });
  await userEvent.setup().click(within(relationships).getByText(/Relationships using this source/));
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
  render(<SourceWorkspace {...props} onOpenLink={onOpenLink} readerOnly />);
  await userEvent.setup().click(await screen.findByRole("button", { name: /Open original/ }));
  await waitFor(() => expect(onOpenLink).toHaveBeenCalledWith("https://issuer.example/filings/2025-q3.pdf"));
  expect(external).not.toHaveBeenCalled();
  await userEvent.setup().click(screen.getByRole("button", { name: "Add to question" }));
  expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ recordId: props.location.recordId, revisionId: props.location.revisionId, spanId: expect.any(String) }));
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
  const [locale, expected] = ["ko", "2026. 10. 27."];
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
    ...record.objects[0], title: "What you follow", titleKey: "followed" as const, originLabel: "Your watchlist and holdings",
  }] };
  const catalog = { ...props.catalog, sources: [followed] };
  const location = { loupistId: props.location.loupistId, surface: "source" as const };
  const view = render(<SourceWorkspace {...props} catalog={catalog} location={location} />);
  expect(screen.getByRole("button", { name: /What you follow.*Your watchlist and holdings/ })).toBeVisible();
  try {
    await act(async () => { await i18n.changeLanguage("ko"); });
    view.rerender(<SourceWorkspace {...props} catalog={catalog} location={location} />);
    const row = screen.getByRole("button", { name: /관심 자산/ });
    expect(row).toHaveTextContent("관심 목록과 보유 자산");
    expect(row).not.toHaveTextContent("What you follow");
    // The title the user reads is the one they can search for.
    view.rerender(<SourceWorkspace {...props} catalog={catalog} location={location} query="관심" />);
    expect(screen.getByRole("button", { name: /관심 자산/ })).toBeVisible();
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});
