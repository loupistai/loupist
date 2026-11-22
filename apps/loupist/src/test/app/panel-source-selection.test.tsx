import { fireEvent, screen, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { workspaceOverlayMedia } from "../../components/workspace/useWorkspacePanelResize";
import { citedFiling } from "../citedFiling";
import { openApp } from "../renderApp";

it.each([[false, false], [true, false], [true, true]])("adds a host-resolved Source selection without sending, then submits its typed offsets (overlay: %s, cited: %s)", async (overlay, cited) => {
  const matchMedia = vi.mocked(window.matchMedia);
  const previous = matchMedia.getMockImplementation();
  matchMedia.mockImplementation((query: string) => ({ matches: overlay && query === workspaceOverlayMedia, media: query, onchange: null, addEventListener: vi.fn(), removeEventListener: vi.fn(), addListener: vi.fn(), removeListener: vi.fn(), dispatchEvent: vi.fn() }));
  try {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
    const source = catalog.sources[0];
    await bridge.workspaceUiWrite({ version: 1, open: true, placement: "side", width: 480, height: 360, sessions: [{ loupist_id: "loupist-quality", thread_id: "thread-margin", active_tab_id: "source", tabs: [{ id: "source", kind: "memory", title: source.title, record_id: source.recordId, location: { loupistId: "loupist-quality", surface: "source", recordId: source.recordId, revisionId: source.revisionId, selection: { kind: "source", sourceId: source.objects[0].id } } }] }] });
    if (cited) {
      const read = bridge.knowledgeSourceRead.bind(bridge);
      vi.spyOn(bridge, "knowledgeSourceRead").mockImplementation(async request => ({
        ...await read(request.spanId === citedFiling.spanId ? { ...request, spanId: undefined } : request),
        material: "provider_payload", selectedSpanId: citedFiling.spanId, spans: [citedFiling],
      }));
    }
    const resolve = vi.spyOn(bridge, "knowledgeSourceSelectionResolve");
    const send = vi.spyOn(bridge, "chatSend");
    await openApp(bridge);
    const passage = await waitFor(() => {
      const selector = ".workspace-panel [data-quote-retained-source]" + (cited ? '[data-cited-passage="1"]' : "");
      const element = document.querySelector<HTMLElement>(selector);
      expect(element).not.toBeNull(); return element!;
    });
    const range = document.createRange();
    if (cited) range.selectNodeContents(passage);
    else {
      range.setStart(passage.firstChild!, 4);
      range.setEnd(passage.firstChild!, 28);
    }
    const exact = range.toString();
    const startUtf16 = cited ? citedFiling.presentation!.text.indexOf(exact) : 4;
    const offsets = { startUtf16, endUtf16: startUtf16 + exact.length, text: exact };
    document.getSelection()!.removeAllRanges(); document.getSelection()!.addRange(range);
    fireEvent.mouseUp(document);
    const addSelection = await waitFor(() => {
      const action = document.querySelector<HTMLButtonElement>(".quote-ask");
      expect(action).toHaveTextContent("Add to question"); return action!;
    });
    fireEvent.click(addSelection);
    const tray = await screen.findByRole("list", { name: "Added to this message" });
    expect(tray.querySelector('[data-kind="source_span"]')).not.toBeNull();
    expect(tray.querySelector('[data-kind="quote"]')).toBeNull();
    expect(resolve).toHaveBeenCalledWith({ source: expect.objectContaining({ recordId: source.recordId, revisionId: source.revisionId, spanId: expect.any(String), purpose: "renderer_inspection" }), selection: offsets });
    expect(send).not.toHaveBeenCalled();
    const composer = screen.getByRole("textbox", { name: "Message Loupist" });
    await waitFor(() => expect(composer).toHaveFocus());
    expect((composer as HTMLTextAreaElement).value).toContain(exact);
    fireEvent.change(composer, { target: { value: `${(composer as HTMLTextAreaElement).value}Explain this passage` } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    expect(send.mock.calls[0][0]).toMatchObject({ prompt: expect.stringContaining("Explain this passage"), pins: [{ kind: "source", recordId: source.recordId, revisionId: source.revisionId, spanId: expect.any(String), selection: offsets }] });
    await waitFor(() => expect(screen.queryByRole("list", { name: "Added to this message" })).not.toBeInTheDocument());
  } finally { if (previous) matchMedia.mockImplementation(previous); }
});
