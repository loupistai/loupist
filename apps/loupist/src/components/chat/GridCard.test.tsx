import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AssetsGrid, ChatArtifactView, ChatEvidenceRef, ConflictGrid, NumberCheck } from "../../types";
import { GRID_SCHEMA } from "../../contracts/chat";
import { ChatGridBlock, columnFit, ConflictCard, GridCard, headerLabel } from "./GridCard";
import { ChatExecutionProvider } from "./ChatExecutionContext";
import { renderWithOpen, destinations } from "../../test/open";
import type { LoupistBridge } from "../../types";

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
    expect(screen.getByText(/2 assets × 2 measures · 3 linked/u)).toBeVisible();
    expect(screen.getByRole("button", { name: "1 number to check" })).toBeVisible();
    expect(screen.getByText(/Known as of Apr 1, 2026/u)).toBeVisible();
    expect(screen.getByRole("region", { name: "Table, scroll sideways" })).toHaveAttribute("tabindex", "0");
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

  it("shows each report with its date, why they differ and which one is used, and links the Map only for a disputed entity", async () => {
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
    expect(screen.queryByRole("button", { name: "See on the Map" })).toBeNull();

    rerender(card({ ...conflict, world_entity_id: "ent:solsup", map_link: true }));
    await user.click(screen.getByRole("button", { name: "See on the Map" }));
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
