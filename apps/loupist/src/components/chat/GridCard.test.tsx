import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AssetsGrid, ChatArtifactView, ChatEvidenceRef, ChatMessage, ConflictGrid, NumberCheck } from "../../types";
import { gridBlocks } from "../../chat/chartBlocks";
import { GRID_SCHEMA } from "../../contracts/chat";
import { ChatGridBlock, columnFit, ConflictCard, GridCard, headerLabel, sharedPeriod } from "./GridCard";
import { ChatExecutionProvider } from "./ChatExecutionContext";
import { renderWithOpen, destinations } from "../../test/open";
import type { LoupistBridge } from "../../types";
import type { ChatDestination, Open } from "../../chat/navigation";
import type { Mock } from "vitest";
import { ArtifactContent } from "../artifacts/ArtifactContent";
import { ChatMessageCard } from "./ChatMessageCard";
import { TooltipProvider } from "../ui/tooltip";

// The thread's scroll actions: revealing something in the answer keeps the page there.
const scroll = vi.hoisted(() => ({ holdScrollPosition: vi.fn(), jumpToLatest: vi.fn(), blockAppended: vi.fn() }));
vi.mock("../ai-elements/conversation", async (load) => ({ ...(await load<typeof import("../ai-elements/conversation")>()), useConversationScroll: () => scroll }));

/** What a component handed up to the app, of one kind. */
const opened = <K extends ChatDestination["kind"]>(open: Mock<Open>, kind: K) =>
  open.mock.calls.map(([to]) => to).filter((to): to is Extract<ChatDestination, { kind: K }> => to.kind === kind);

const DART: ChatEvidenceRef = {
  id: "ev_dart", title: "Solsup Foods FY2025 statements", kind: "original", provider: "OpenDART", tool_name: "dart_financials",
  retrieved_at: "2026-03-20T06:00:00Z", published_at: "2026-03-18", excerpt: "", excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
};
const NEWS: ChatEvidenceRef = { ...DART, id: "ev_news", title: "News report, Hanbada Daily", provider: undefined, tool_name: "web_fetch", published_at: "2026-02-05", excerpt: "Preliminary revenue of ₩455.0bn." };
const check = (text: string, status: NumberCheck["status"] = "rounded", evidence = DART.id): NumberCheck => ({ link_index: 0, text, evidence_id: evidence, kind: "data", status });

const grid: AssetsGrid = {
  layout: "assets",
  rows: [
    { symbol: "990221", market: "KRX", label: "Solsup Foods", asset_id: "ast_0123456789abcdef" },
    { symbol: "990777", market: "KRX", label: "Hanbit Chem" },
  ],
  columns: [
    { id: "revenue", label: "Revenue", period: "FY2025", unit: "amount" },
    { id: "payout", label: "Payout from earnings", period: "FY2025", unit: "percent" },
  ],
  cells: [
    [{ value: "₩452.3bn", evidence_id: DART.id, check: check("₩452.3bn"), sort: 452.3e9 }, { value: "42.0%", evidence_id: DART.id, check: check("42.0%", "differs"), sort: 42 }],
    [{ value: "₩286.5bn", evidence_id: DART.id, check: check("₩286.5bn"), sort: 286.5e9 }, { empty: "not_reported", note: "Separate statements only." }],
  ],
  as_of: "2026-04-01",
};

/** When the message holding the table was written. */
const SAVED_AT = "2026-04-01T09:30:00Z";

function renderGrid(props: Partial<Parameters<typeof GridCard>[0]> = {}) {
  return render(<GridCard title="Dividend payers" grid={grid} evidenceRefs={[DART]} {...props} />);
}

describe("GridCard", () => {
  it("keeps copy feedback across equivalent grid snapshots", async () => {
    const user = userEvent.setup();
    const view = renderGrid();
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(screen.getByRole("button", { name: "Copied" })).toBeVisible();
    view.rerender(<GridCard title="Dividend payers" grid={structuredClone(grid)} evidenceRefs={[DART]} />);
    expect(screen.getByRole("button", { name: "Copied" })).toBeVisible();
  });

  it("is a real table: values are number chips, empty cells speak their reason, the first column names the asset", () => {
    renderGrid();
    const table = screen.getByRole("table", { name: "Dividend payers" });
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(3);
    const asset = within(rows[1]!).getByRole("rowheader");
    expect(asset).toHaveClass("grid-card-asset");
    expect(asset).toHaveTextContent("990221 · KRX");
    // Values reuse the number chip and its popover.
    expect(within(table).getByRole("button", { name: /₩452\.3bn, from OpenDART/u })).toHaveAttribute("data-status", "rounded");
    expect(within(table).getByText("42.0%").closest("button")).toHaveAttribute("data-status", "differs");
    // The empty reason is spoken with its row and column, never as a zero.
    expect(within(table).getByRole("button", { name: "Hanbit Chem, Payout from earnings: Not reported" })).toHaveTextContent("Not reported");
    expect(screen.getByText(/2 assets × 2 measures · 3 values with sources/u)).toBeVisible();
    expect(screen.getByRole("button", { name: "1 number to check" })).toBeVisible();
    expect(screen.getByText(/Known as of Apr 1, 2026/u)).toBeVisible();
    expect(screen.getByRole("region", { name: "Table, scroll sideways" })).toHaveAttribute("tabindex", "0");
  });

  it("describes an empty cell by its note, and keeps the note open while the pointer crosses into it", async () => {
    vi.useFakeTimers();
    try {
      renderGrid();
      const cell = screen.getByRole("button", { name: "Hanbit Chem, Payout from earnings: Not reported" });
      expect(cell).toHaveAccessibleDescription("Separate statements only.");
      const note = () => document.querySelector<HTMLElement>(".grid-card-note");
      fireEvent.pointerEnter(cell);
      expect(note()).toHaveTextContent("Separate statements only.");
      fireEvent.pointerLeave(cell);
      fireEvent.pointerEnter(note()!);
      act(() => { vi.advanceTimersByTime(1000); });
      expect(note()).toBeVisible();
      fireEvent.pointerLeave(note()!);
      act(() => { vi.advanceTimersByTime(1000); });
      expect(note()).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps an empty cell's note open when the cell is clicked, and closes it on Escape", async () => {
    const user = userEvent.setup();
    renderGrid();
    const cell = screen.getByRole("button", { name: "Hanbit Chem, Payout from earnings: Not reported" });
    await user.click(cell);
    await waitFor(() => expect(document.querySelector(".grid-card-note")).toHaveTextContent("Separate statements only."));
    await user.keyboard("{Escape}");
    await waitFor(() => expect(document.querySelector(".grid-card-note")).toBeNull());
  });

  it("opens the first number to check from the caption and keeps the thread there", async () => {
    renderGrid();
    await userEvent.setup().click(screen.getByRole("button", { name: "1 number to check" }));
    expect(await screen.findByRole("dialog")).toHaveTextContent(/The calculation gave/u);
    expect(scroll.holdScrollPosition).toHaveBeenCalled();
  });

  it("shows the values it rounds in a column with the same decimals, and its chip says the shown value was rounded", () => {
    const amount = (value: string) => ({ value, evidence_id: DART.id, check: { ...check(value, "matched"), exact: value }, sort: 0 });
    const rounded: AssetsGrid = { ...grid, cells: [[amount("₩47.214321조"), grid.cells[0]![1]!], [amount("₩43.600123조"), grid.cells[1]![1]!]] };
    renderGrid({ grid: rounded });
    const table = screen.getByRole("table", { name: "Dividend payers" });
    expect(within(table).getByRole("button", { name: /^₩47\.21조, from OpenDART/u })).toHaveAttribute("data-status", "rounded");
    expect(within(table).getByRole("button", { name: /^₩43\.60조, from OpenDART/u })).toHaveAttribute("data-status", "rounded");
    expect(within(table).queryByText("₩43.6조")).toBeNull();
  });

  it("sorts by a column header and keeps empty cells last", async () => {
    const user = userEvent.setup();
    renderGrid();
    const header = screen.getByRole("columnheader", { name: /Revenue/u });
    expect(header).toHaveAttribute("aria-sort", "none");
    await user.click(within(header).getByRole("button"));
    expect(header).toHaveAttribute("aria-sort", "descending");
    await user.click(within(header).getByRole("button"));
    expect(header).toHaveAttribute("aria-sort", "ascending");
    expect(screen.getAllByRole("rowheader").map((cell) => cell.querySelector(".grid-card-name")?.textContent)).toEqual(["Hanbit Chem", "Solsup Foods"]);
    const payout = screen.getByRole("columnheader", { name: /Payout/u });
    await user.click(within(payout).getByRole("button"));
    await user.click(within(payout).getByRole("button"));
    // Low to high: the empty cell still sorts last.
    expect(screen.getAllByRole("rowheader").map((cell) => cell.querySelector(".grid-card-name")?.textContent)).toEqual(["Solsup Foods", "Hanbit Chem"]);
    expect(header).toHaveAttribute("aria-sort", "none");
  });

  it("exports CSV with sources through the native save and says when saving failed", async () => {
    const user = userEvent.setup();
    const onExport = vi.fn().mockRejectedValueOnce(new Error("disk full")).mockResolvedValue("/tmp/Dividend payers.csv");
    renderGrid({ onExport });
    await user.click(screen.getAllByRole("button", { name: /Export/u })[0]!);
    await user.click(await screen.findByRole("menuitem", { name: "CSV with sources" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't save the file.");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(onExport).toHaveBeenLastCalledWith(expect.objectContaining({ suggestedName: "Dividend payers.csv", kind: "csv" }));
    const bytes = onExport.mock.calls.at(-1)![0].bytes as number[];
    const text = new TextDecoder("utf-8", { ignoreBOM: true }).decode(new Uint8Array(bytes));
    expect(text.startsWith("﻿asset,symbol,market,measure")).toBe(true);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("folds Copy, both exports and Ask into one menu for a narrow window", async () => {
    const user = userEvent.setup();
    renderGrid({ onExport: vi.fn(), onAsk: vi.fn() });
    // Below 720 px the stylesheet shows this menu in place of the action buttons.
    await user.click(screen.getByRole("button", { name: "More actions", hidden: true }));
    const items = (await screen.findAllByRole("menuitem")).map((item) => item.textContent);
    expect(items).toEqual(["Copy", "CSV with sources", "Markdown table", "Ask"]);
  });
});

describe("GridCard asset names", () => {
  const scope = { bridge: {} as LoupistBridge, loupistId: "loupist-a", threadId: "chat", surface: "session" as const, theme: "light" as const, active: true };
  const value = (text: string) => ({ value: text, evidence_id: DART.id, check: check(text), sort: 1 });
  // A linked saved row, an exchange-style row saved without an id (its symbol has a dot), and a comparison label.
  const named: AssetsGrid = {
    layout: "assets",
    rows: [
      { symbol: "990221", market: "KRX", label: "Solsup Foods", asset_id: "ast_0123456789abcdef" },
      { symbol: "BRK.B", market: "NYSE", label: "Berkshire Hathaway" },
      { symbol: "Peer median", market: "Peers", label: "Peer median" },
    ],
    columns: [{ id: "revenue", label: "Revenue", period: "FY2025", unit: "amount" }],
    cells: [[value("₩452.3bn")], [value("$371.4bn")], [value("₩300.0bn")]],
    as_of: "2026-04-01",
  };
  const card = <GridCard title="Revenue" grid={named} evidenceRefs={[DART]} />;

  it("opens a row by its stored id, else by its ticker, and leaves a comparison label as text", async () => {
    const user = userEvent.setup();
    const { open } = renderWithOpen(<ChatExecutionProvider value={scope}>{card}</ChatExecutionProvider>);
    await user.click(screen.getByRole("button", { name: "Berkshire Hathaway" }));
    // An id-less exchange-style row opens by its ticker; the host says when nothing is tracked under it.
    expect(destinations(open)).toEqual([{ kind: "asset", target: { loupistId: "loupist-a", ticker: "BRK.B", name: "Berkshire Hathaway" } }]);
    screen.getByRole("button", { name: "Berkshire Hathaway" }).focus();
    await user.keyboard("{Enter}");
    expect(destinations(open)).toHaveLength(2);
    await user.click(screen.getByRole("button", { name: "Solsup Foods" }));
    expect(destinations(open).at(-1)).toEqual({ kind: "asset", target: { loupistId: "loupist-a", assetId: "ast_0123456789abcdef", name: "Solsup Foods" } });
    expect(screen.queryByRole("button", { name: "Peer median" })).toBeNull();
    expect(screen.getByText("Peer median", { selector: "span.grid-card-name" })).toBeVisible();
    expect(screen.getByText("BRK.B · NYSE")).toBeVisible();
  });

  it("opens a detached table's row by its stored id", async () => {
    const user = userEvent.setup();
    const revisionMs = Date.parse("2026-02-03T04:05:06Z");
    const view = {
      artifact: { id: "grid-1", chat_session_id: "chat", kind: "grid", title: "Revenue", current_revision: 3, created_at_ms: 1, updated_at_ms: Date.now() },
      revision: { artifact_id: "grid-1", revision: 2, digest: "c".repeat(64), source_message_id: "answer", created_at_ms: revisionMs, payload: { kind: "grid", schema: GRID_SCHEMA, grid: named } },
      evidence_refs: [DART],
    } as ChatArtifactView;
    const { open } = renderWithOpen(<ChatExecutionProvider value={scope}><ArtifactContent view={view} theme="light" /></ChatExecutionProvider>);
    await user.click(screen.getByRole("button", { name: "Solsup Foods" }));
    expect(destinations(open)).toEqual([{ kind: "asset", target: { loupistId: "loupist-a", assetId: "ast_0123456789abcdef", name: "Solsup Foods" } }]);
  });

  it("opens a reloaded table's row in a real answer, and keeps its menu actions", async () => {
    const user = userEvent.setup({ skipHover: true });
    const ref = { artifact_id: "grid-1", revision: 1, kind: "grid" as const, title: "Revenue", digest: "c".repeat(64) };
    const stored = {
      artifact: { id: "grid-1", chat_session_id: "chat", kind: "grid", title: "Revenue", current_revision: 1, created_at_ms: 0, updated_at_ms: 0 },
      revision: { artifact_id: "grid-1", revision: 1, digest: "c".repeat(64), source_message_id: "answer", created_at_ms: Date.now(), payload: { kind: "grid", schema: GRID_SCHEMA, grid: named } },
      evidence_refs: [DART],
    } as ChatArtifactView;
    const chatArtifactRead = vi.fn(async () => stored);
    const scoped = { ...scope, bridge: { chatArtifactRead } as unknown as LoupistBridge };
    const message: ChatMessage = { id: "answer", role: "assistant", status: "complete", content: "Here is the table.", created_at: SAVED_AT, artifact_refs: [ref] };
    const { open } = renderWithOpen(<TooltipProvider><ChatExecutionProvider value={scoped}>
      <ChatMessageCard message={message} loupistId="loupist-a" chatId="chat" loupistName="Loupist" onReadAttachment={vi.fn()} onAskArtifact={vi.fn()} />
    </ChatExecutionProvider></TooltipProvider>);
    await user.click(await screen.findByRole("button", { name: "Berkshire Hathaway" }));
    expect(opened(open, "asset")).toEqual([{ kind: "asset", target: { loupistId: "loupist-a", ticker: "BRK.B", name: "Berkshire Hathaway" } }]);
    expect(chatArtifactRead).toHaveBeenCalledWith("loupist-a", "chat", "grid-1", 1);
    // The card's own actions still work in the answer.
    expect(screen.getByRole("table", { name: "Revenue" })).toBeVisible();
  });

  it("opens a staged table's names while the answer streams and after the host saves it", async () => {
    const user = userEvent.setup({ skipHover: true });
    const view = (digest: string): ChatArtifactView => ({
      artifact: { id: "grid-1", chat_session_id: "chat", kind: "grid", title: "Revenue", current_revision: 1, created_at_ms: 0, updated_at_ms: 0 },
      revision: { artifact_id: "grid-1", revision: 1, digest, source_message_id: "answer", created_at_ms: 0, payload: { kind: "grid", schema: GRID_SCHEMA, grid: named } },
      evidence_refs: [DART],
    } as ChatArtifactView);
    const staged = view("c".repeat(64));
    const scoped = { ...scope, bridge: { chatArtifactRead: vi.fn(async () => staged) } as unknown as LoupistBridge };
    const step = { id: "publish", kind: "tool" as const, toolName: "grid_publish", category: "artifact" as const, operation: "publish" as const,
      action: "Made a table", target: "Revenue", href: null, status: "succeeded" as const, startedAtMs: 0, finishedAtMs: null };
    // While streaming, `created_at` is the renderer's clock; the host's own time arrives with completion.
    const renderer = "2026-11-18T09:00:00.000Z", host = "2026-11-18T09:00:07.250Z";
    const streaming: ChatMessage = { id: "answer", role: "assistant", status: "streaming", content: "", created_at: renderer, staged_artifacts: [staged], progress: { startedAtMs: 0, finishedAtMs: null, items: [step] } };
    const answer = (message: ChatMessage) => <TooltipProvider><ChatExecutionProvider value={scoped}>
      <ChatMessageCard message={message} loupistId="loupist-a" chatId="chat" loupistName="Loupist" onReadAttachment={vi.fn()} />
    </ChatExecutionProvider></TooltipProvider>;
    const { open, rerender } = renderWithOpen(answer(streaming));
    const table = await screen.findByRole("table", { name: "Revenue" });
    await user.click(within(table).getByRole("button", { name: "Berkshire Hathaway" }));
    rerender(answer({ ...streaming, status: "complete", created_at: host, artifact_refs: [{ artifact_id: "grid-1", revision: 1, kind: "grid", title: "Revenue", digest: "c".repeat(64) }] }));
    await user.click(await screen.findByRole("button", { name: "Berkshire Hathaway" }));
    expect(opened(open, "asset")).toEqual([1, 2].map(() => ({ kind: "asset", target: { loupistId: "loupist-a", ticker: "BRK.B", name: "Berkshire Hathaway" } })));
  });

  it("names every row as text without a scope or an opener", () => {
    const unscoped = renderWithOpen(card);
    expect(within(screen.getByRole("table")).queryAllByRole("button", { name: /Solsup Foods|Berkshire Hathaway|Peer median/u })).toEqual([]);
    unscoped.unmount();
    render(<ChatExecutionProvider value={scope}>{card}</ChatExecutionProvider>);
    expect(within(screen.getByRole("table")).queryAllByRole("button", { name: /Solsup Foods|Berkshire Hathaway|Peer median/u })).toEqual([]);
    expect(screen.getByText("Berkshire Hathaway", { selector: "span.grid-card-name" })).toBeVisible();
  });
});

it("uses the grid row's ticker for a quote cell and does not match by price", async () => {
  const user = userEvent.setup();
  const excerpt = JSON.stringify({ results: [
    { symbol: "AAPL", name: "Apple", currency: "USD", last_price: 271.4 },
    { symbol: "MSFT", name: "Microsoft", currency: "USD", last_price: 271.4 },
  ] });
  const quote: ChatEvidenceRef = {
    id: "ev_quote", title: "Equity price quote", kind: "original", provider: "Yahoo Finance", tool_name: "equity_price_quote",
    retrieved_at: "2026-10-11T15:04:00Z", excerpt, excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
  };
  const quoteGrid: AssetsGrid = {
    layout: "assets",
    rows: [{ symbol: "AAPL", market: "NASDAQ", label: "Apple" }],
    columns: [{ id: "last", label: "Last", unit: "amount" }],
    cells: [[{ value: "271.4", evidence_id: quote.id, check: { link_index: 0, text: "271.4", evidence_id: quote.id, kind: "passage", status: "matched" }, sort: 271.4 }]],
  };
  const { open } = renderWithOpen(<GridCard title="Quotes" grid={quoteGrid} evidenceRefs={[quote]} />);
  await user.click(screen.getByRole("button", { name: /271\.4/ }));
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("AAPL");
  expect(dialog).toHaveTextContent("271.4 USD");
  expect(dialog).not.toHaveTextContent("MSFT");
  expect(open).not.toHaveBeenCalled();
});

it("keeps the pointed quote when the grid row names a different ticker", async () => {
  const user = userEvent.setup();
  const excerpt = JSON.stringify({ results: [
    { symbol: "AAPL", name: "Apple", currency: "USD", last_price: 271.4 },
    { symbol: "MSFT", name: "Microsoft", currency: "USD", last_price: 415.2 },
  ] });
  const quote: ChatEvidenceRef = {
    id: "ev_quote", title: "Equity price quote", kind: "original", provider: "Yahoo Finance", tool_name: "equity_price_quote",
    retrieved_at: "2026-10-11T15:04:00Z", excerpt, excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [],
  };
  const quoteGrid: AssetsGrid = {
    layout: "assets",
    rows: [{ symbol: "MSFT", market: "NASDAQ", label: "Microsoft" }],
    columns: [{ id: "last", label: "Last", unit: "amount" }],
    cells: [[{ value: "271.4", evidence_id: quote.id, check: {
      link_index: 0, text: "271.4", evidence_id: quote.id, kind: "data", status: "matched", pointer: "/results/0/last_price",
      fields: [{ key: "symbol", value: "MSFT" }, { key: "period", value: "FY2025" }, { key: "last_price", value: "271.4", matched: true }],
    }, sort: 271.4 }]],
  };
  render(<GridCard title="Quotes" grid={quoteGrid} evidenceRefs={[quote]} />);
  await user.click(screen.getByRole("button", { name: /271\.4/ }));
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("AAPL");
  expect(dialog).toHaveTextContent("271.4 USD");
  expect(dialog).not.toHaveTextContent("MSFT");
  expect(dialog).not.toHaveTextContent("415.2");
});

describe("ConflictCard", () => {
  const conflict: ConflictGrid = {
    layout: "conflict",
    measure: { label: "revenue", entity_label: "Solsup Foods", period: "FY2025" },
    reports: [
      { evidence_id: DART.id, says: "₩452.3bn", basis: "Consolidated, final", dated: "2026-03-18", check: check("₩452.3bn") },
      { evidence_id: NEWS.id, says: "₩455.0bn", basis: "Preliminary", check: { ...check("₩455.0bn", "matched", NEWS.id), kind: "passage" } },
    ],
    reason: "preliminary_vs_final",
    explanation: "The news report carried the preliminary figure.",
    used: 0,
    used_reason: "The filed figure is final.",
  };

  it("shows each report with its date, why they differ and which one is used, and links World only for a disputed entity", async () => {
    const user = userEvent.setup();
    const scope = { bridge: {} as LoupistBridge, loupistId: "loupist-a", threadId: "chat", surface: "session" as const, theme: "light" as const, active: true };
    const card = (value: ConflictGrid) => <ChatExecutionProvider value={scope}><ConflictCard title="Solsup revenue" conflict={value} evidenceRefs={[DART, NEWS]} /></ChatExecutionProvider>;
    const { rerender, open } = renderWithOpen(card(conflict));
    expect(screen.getByText("Where sources disagree · Solsup Foods · FY2025 revenue", { selector: ".conflict-card-label" })).toBeVisible();
    const rows = screen.getAllByRole("row");
    expect(within(rows[1]!).getByText("Used")).toBeVisible();
    expect(within(rows[1]!).getByText("Mar 18")).toHaveAttribute("title", "Mar 18, 2026");
    expect(within(rows[2]!).getByText("Undated")).toBeVisible();
    expect(within(rows[2]!).queryByText("Used")).toBeNull();
    expect(screen.getByText(/Why they differ: preliminary versus final\. The news report/u)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Show in World" })).toBeNull();

    rerender(card({ ...conflict, world_entity_id: "ent:solsup", map_link: true }));
    await user.click(screen.getByRole("button", { name: "Show in World" }));
    expect(open).toHaveBeenLastCalledWith({ kind: "page", location: { loupistId: "loupist-a", surface: "world", view: { onlyDisputed: true, highlightEntityIds: ["ent:solsup"] } } });
  });

  it("cites each report with a real citation that opens its evidence", async () => {
    const { open } = renderWithOpen(<ConflictCard title="Solsup revenue" conflict={conflict} evidenceRefs={[DART, NEWS]} />);
    const rows = screen.getAllByRole("row");
    const chip = within(rows[1]!).getByRole("button", { name: /Source 1/u });
    await userEvent.setup().click(chip);
    expect(destinations(open).at(-1)).toEqual({ kind: "evidence", evidence: DART });
  });
});

describe("wide grids", () => {
  it("lets the next column peek in and counts the columns still hidden", () => {
    // Four measures fit beside a 176 px asset column: no peek, nothing hidden.
    expect(columnFit(766, 176, 4)).toEqual({ width: undefined, hidden: 0 });
    const wide = columnFit(766, 176, 8);
    // Four whole columns and 45% of a fifth fill the view; the rest are counted.
    expect(wide.width).toBeGreaterThanOrEqual(112);
    expect(wide.width).toBeLessThanOrEqual(168);
    expect((766 - 176) / wide.width! % 1).toBeCloseTo(0.45, 2);
    expect(wide.hidden).toBe(4);
    // Scrolled to the end, nothing is hidden.
    expect(columnFit(766, 176, 8, 8 * wide.width! + 176 - 766).hidden).toBe(0);
  });

  it("never makes a column narrower than its widest value, so a long amount does not run into the next one", () => {
    // Four 112 px columns would fit, but "333,605,938,000,000원" with its padding needs 159 px: the grid scrolls instead.
    const fit = columnFit(766, 176, 4, 0, 159);
    expect(fit.width).toBeGreaterThanOrEqual(159);
    expect(fit.hidden).toBe(1);
    // Roomy enough for every column at that width: the columns share the row.
    expect(columnFit(1000, 176, 4, 0, 159)).toEqual({ width: undefined, hidden: 0 });
    // Wider than a column may be: held at 168 px.
    expect(columnFit(766, 176, 4, 0, 400).width).toBe(168);
  });

  it("says a period and basis every measure shares once, in the header, and each measure's own otherwise", () => {
    const shared: AssetsGrid = { ...grid, subtitle: "Consolidated", columns: grid.columns.map((column) => ({ ...column, basis: "CFS" })) };
    expect(sharedPeriod(shared)).toBe("FY2025 · CFS");
    const { unmount } = renderGrid({ grid: shared });
    expect(screen.getByText("FY2025 · CFS", { selector: ".grid-card-header p" })).toBeVisible();
    expect(screen.getByText("Consolidated", { selector: ".grid-card-header p" })).toBeVisible();
    expect(within(screen.getByRole("table")).queryByText(/FY2025/u)).toBeNull();
    unmount();
    const differs: AssetsGrid = { ...grid, columns: [grid.columns[0]!, { ...grid.columns[1]!, period: "FY2024" }] };
    expect(sharedPeriod(differs)).toBeNull();
    renderGrid({ grid: differs });
    expect(within(screen.getByRole("columnheader", { name: /Revenue/u })).getByText("FY2025")).toBeVisible();
    expect(within(screen.getByRole("columnheader", { name: /Payout/u })).getByText("FY2024")).toBeVisible();
    expect(screen.queryByText(/FY202/u, { selector: ".grid-card-header p" })).toBeNull();
  });

  describe("measures as rows", () => {
    // jsdom lays nothing out: give the card a width, the asset column 176 px and each value 60 px.
    let width = 0;
    beforeEach(() => {
      width = 400;
      vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(function (this: HTMLElement) { return this.classList.contains("grid-card-scroll") ? width : 0; });
      vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(function (this: HTMLElement) { return this.classList.contains("grid-card-asset-head") ? 176 : 60; });
    });
    afterEach(() => vi.restoreAllMocks());
    const three: AssetsGrid = {
      ...grid,
      columns: [...grid.columns, { id: "yield", label: "Dividend yield", period: "Oct 31", unit: "percent" }],
      cells: [[...grid.cells[0]!, { value: "3.1%", evidence_id: DART.id, check: check("3.1%"), sort: 3.1 }], [...grid.cells[1]!, { empty: "unavailable" }]],
    };

    it("turns a table with more measures than assets that does not fit: measures are rows, assets are columns, nothing sorts", () => {
      renderGrid({ grid: three });
      const table = screen.getByRole("table", { name: "Dividend payers" });
      expect(table).toHaveAttribute("data-rows", "measures");
      const heads = within(table).getAllByRole("columnheader");
      expect(heads.map((head) => head.textContent)).toEqual(["Measure", "Solsup Foods990221 · KRX", "Hanbit Chem990777 · KRX"]);
      expect(heads.every((head) => head.getAttribute("scope") === "col")).toBe(true);
      const rows = within(table).getAllByRole("rowheader");
      expect(rows.map((head) => [head.getAttribute("scope"), head.textContent])).toEqual([
        ["row", "RevenueFY2025"], ["row", "Payout from earningsFY2025"], ["row", "Dividend yieldOct 31"],
      ]);
      expect(within(table).queryByRole("button", { name: /Revenue/u })).toBeNull();
      expect(table.querySelector("[aria-sort]")).toBeNull();
      // Cells are unchanged: chips with their status, empty reasons spoken with asset and measure.
      expect(within(table).getByText("42.0%").closest("button")).toHaveAttribute("data-status", "differs");
      expect(within(table).getByRole("button", { name: "Hanbit Chem, Payout from earnings: Not reported" })).toBeVisible();
      // One row per measure, its cells in asset order.
      expect(within(table).getAllByRole("row")[3]!.textContent).toMatch(/^Dividend yieldOct 313\.1%.*Hanbit Chem, Dividend yield: Couldn't get$/u);
      expect(screen.queryByRole("button", { name: /more column/u })).toBeNull();
    });

    it("keeps asset rows when they fit, or when the table has no more measures than assets", () => {
      width = 900;
      const { unmount } = renderGrid({ grid: three });
      expect(screen.getByRole("table")).toHaveAttribute("data-rows", "assets");
      unmount();
      width = 200;
      renderGrid();
      expect(screen.getByRole("table")).toHaveAttribute("data-rows", "assets");
    });

    it("decides on the width the asset rows need, so a resize turns it once each way", () => {
      renderGrid({ grid: three });
      const region = screen.getByRole("region", { name: "Table, scroll sideways" });
      // 176 px + 3 × 112 px = 512 px: anything narrower turns, anything at least that wide does not.
      for (const [next, rows] of [[511, "measures"], [512, "assets"], [512, "assets"], [511, "measures"], [400, "measures"]] as const) {
        width = next;
        fireEvent.scroll(region);
        expect(screen.getByRole("table")).toHaveAttribute("data-rows", rows);
      }
    });

    it("copies and exports asset rows in either layout", async () => {
      const user = userEvent.setup();
      const onExport = vi.fn().mockResolvedValue("/tmp/t.csv");
      const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
      const saved = async () => {
        await user.click(screen.getAllByRole("button", { name: /Export/u })[0]!);
        await user.click(await screen.findByRole("menuitem", { name: "Markdown table" }));
        await user.click(screen.getAllByRole("button", { name: "Copy" })[0]!);
        return [onExport.mock.calls.at(-1)![0].bytes, writeText.mock.calls.at(-1)![0]];
      };
      const { unmount } = renderGrid({ grid: three, onExport });
      expect(screen.getByRole("table")).toHaveAttribute("data-rows", "measures");
      const turned = await saved();
      unmount();
      width = 900;
      renderGrid({ grid: three, onExport });
      expect(screen.getByRole("table")).toHaveAttribute("data-rows", "assets");
      expect(await saved()).toEqual(turned);
      expect(String(turned[1]).split("\n")[1]).toMatch(/^Solsup Foods/u);
    });
  });

  it("breaks a slash term only after its slash", () => {
    expect(headerLabel("Net debt/EBITDA")).toBe("Net\u00a0debt/\u200bEBITDA");
    expect(headerLabel("Operating margin")).toBe("Operating margin");
  });
});

it("never draws a Loading frame between a preparing table and its delivered content", async () => {
  const scope = { bridge: {} as LoupistBridge, loupistId: "loupist-a", threadId: "chat", surface: "session" as const, theme: "light" as const, active: true };
  const staged: ChatArtifactView = {
    artifact: { id: "grid-1", chat_session_id: "chat", kind: "grid", title: "Dividend payers", current_revision: 1, created_at_ms: 0, updated_at_ms: 0 },
    revision: { artifact_id: "grid-1", revision: 1, digest: "c".repeat(64), source_message_id: "answer", created_at_ms: 0, payload: { kind: "grid", schema: GRID_SCHEMA, grid } },
    evidence_refs: [DART],
  } as ChatArtifactView;
  const ref = { artifact_id: "grid-1", revision: 1, kind: "grid" as const, title: "Dividend payers", digest: "c".repeat(64) };
  const { container, rerender } = render(<ChatExecutionProvider value={scope}><ChatGridBlock block={{ kind: "building", key: "grid:grid-1", title: "Dividend payers", stepId: "step" }} /></ChatExecutionProvider>);
  const added: string[] = [];
  const observer = new MutationObserver((records) => { for (const record of records) for (const node of record.addedNodes) added.push(node.textContent ?? ""); });
  observer.observe(container, { childList: true, subtree: true, characterData: true });
  rerender(<ChatExecutionProvider value={scope}><ChatGridBlock block={{ kind: "ready", key: "grid:grid-1", artifact: ref, updating: false, staged }} /></ChatExecutionProvider>);
  await Promise.resolve();
  observer.disconnect();
  expect(added.join("|")).not.toMatch(/Loading/);
  expect(screen.getByRole("table", { name: "Dividend payers" })).toBeVisible();
});

it("replaces a table corrected in the same reply in place: the corrected value is on screen and the first is gone", async () => {
  const draft = (digest: string, payout: string): ChatArtifactView => ({
    artifact: { id: "grid-1", chat_session_id: "chat", kind: "grid", title: "Dividend payers", current_revision: 1, created_at_ms: 0, updated_at_ms: 0 },
    revision: { artifact_id: "grid-1", revision: 1, digest, source_message_id: "answer", created_at_ms: 0, payload: { kind: "grid", schema: GRID_SCHEMA, grid: {
      ...grid, cells: [[grid.cells[0]![0]!, { value: payout, evidence_id: DART.id, check: check(payout, "matched"), sort: Number.parseFloat(payout) }], grid.cells[1]!],
    } } },
    evidence_refs: [DART],
  } as ChatArtifactView);
  const first = draft("a".repeat(64), "42.0%");
  const corrected = draft("b".repeat(64), "40.2%");
  const chatArtifactRead = vi.fn(async () => corrected);
  const scope = { bridge: { chatArtifactRead } as unknown as LoupistBridge, loupistId: "loupist-a", threadId: "chat", surface: "session" as const, theme: "light" as const, active: true };
  const step = (id: string) => ({ id, kind: "tool" as const, toolName: "grid_publish", category: "artifact" as const, operation: "publish" as const,
    action: "Made a table", target: "Dividend payers", href: null, status: "succeeded" as const, startedAtMs: 0, finishedAtMs: null });
  const message = (extra: Partial<ChatMessage>): ChatMessage => ({ id: "answer", role: "assistant", content: "", created_at: "2026-11-17T00:00:00Z", status: "streaming",
    progress: { startedAtMs: 0, finishedAtMs: null, items: [step("publish"), step("revise")] }, ...extra });
  const blockOf = (value: ChatMessage) => <ChatExecutionProvider value={scope}><ChatGridBlock block={gridBlocks(value)[0]!} /></ChatExecutionProvider>;
  const { rerender } = render(blockOf(message({ staged_artifacts: [first] })));
  const table = () => screen.getByRole("table", { name: "Dividend payers" });
  expect(within(table()).getByText("42.0%")).toBeVisible();
  rerender(blockOf(message({ staged_artifacts: [corrected] })));
  expect(screen.getAllByRole("table")).toHaveLength(1);
  expect(within(table()).getByText("40.2%")).toBeVisible();
  expect(within(table()).queryByText("42.0%")).toBeNull();
  // Committed, the card reads the stored revision as any stored card does, and keeps the corrected table meanwhile.
  const ref = { artifact_id: "grid-1", revision: 1, kind: "grid" as const, title: "Dividend payers", digest: "b".repeat(64) };
  rerender(blockOf(message({ status: "complete", artifact_refs: [ref], staged_artifacts: [corrected] })));
  expect(within(table()).getByText("40.2%")).toBeVisible();
  await waitFor(() => expect(chatArtifactRead).toHaveBeenCalledWith("loupist-a", "chat", "grid-1", 1));
  expect(within(table()).getByText("40.2%")).toBeVisible();
  expect(within(table()).queryByText("42.0%")).toBeNull();
});
