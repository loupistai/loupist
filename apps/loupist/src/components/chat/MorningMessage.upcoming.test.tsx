import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "../../i18n";
import type { UpcomingEvent } from "../../contracts/assets";
import type { JournalRecord, KnowledgeMap } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { AssetPageOpenerProvider } from "../assets/assetOpener";
import { MorningMessage } from "./MorningMessage";

const HOUR = 3_600_000;
// Saturday; the last visit was Thursday.
const NOW = new Date(2026, 9, 31, 9, 0).getTime();
const LAST_SEEN = NOW - 2 * 24 * HOUR;

const map = { nodes: [
  { id: "hanul", kind: "entity", label: "Hanul Precision", localNames: ["한울정밀"], entityKind: "Organization", degree: 2, watched: true, held: false, recordId: "world-1" },
], edges: [] } as unknown as KnowledgeMap;
const journal = [
  { journalId: "j1", judgment: "Inventory falls", archived: false, check: { claim: "Inventory days below 50", metric: "inventory days", direction: "below", threshold: 50, byDate: "2026-10-29" } },
  { journalId: "j2", judgment: "Margin holds", archived: false, check: { claim: "Gross margin stays above 55%", metric: "gross margin", direction: "above", threshold: 55, byDate: "2026-11-03" } },
] as JournalRecord[];
const asset = (assetId: string, symbol: string, market: string, name: string, entityId?: string) => ({ assetId, symbol, market, name, ...(entityId ? { entityId } : {}) });
const morrow = asset("ast_00000000000000a1", "MORR", "NASDAQ", "Morrow Components");
const hanul = asset("ast_00000000000000a2", "990308", "KRX", "Hanul Precision", "hanul");
const events: UpcomingEvent[] = [
  { kind: "results", date: "2026-11-02", asset: morrow, dateSource: "calendar", eventId: "ern_00000000000000b1", period: "Q3 2026" },
  { kind: "check", date: "2026-11-03", journalId: "j2", claim: "Gross margin stays above 55%" },
  { kind: "results", date: "2026-11-06", asset: hanul, dateSource: "legal_deadline", eventId: "ern_00000000000000b2", period: "Q3 2026" },
  { kind: "recordDate", date: "2026-11-09", asset: asset("ast_00000000000000a3", "BLUE", "NYSE", "Bluehaven Memory") },
];

function bridge(upcoming: UpcomingEvent[] = events) {
  return {
    knowledgeMap: vi.fn(async () => map),
    knowledgeGrowthEvents: vi.fn(async () => ({ events: [], nextCursor: null })),
    inboxList: vi.fn(async () => ({ items: [], nextCursor: null })),
    journalList: vi.fn(async () => journal),
    watchlistList: vi.fn(async () => []),
    upcomingEvents: vi.fn(async () => upcoming),
  } as unknown as LoupistBridge;
}

function renderMessage(source = bridge(), props: { asOf?: string; mode?: "full" | "questions" } = {}) {
  const handlers = { onAsk: vi.fn(), onOpenMapNode: vi.fn(), onOpenMap: vi.fn(), onOpenJournal: vi.fn() };
  const openAsset = vi.fn();
  render(<AssetPageOpenerProvider value={openAsset}>
    <MorningMessage bridge={source} loupistId="loupist" loupistName="Quality Compounder" {...handlers} {...props} />
  </AssetPageOpenerProvider>);
  return { handlers, openAsset, source };
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.setSystemTime(NOW);
  localStorage.setItem("loupist.loupistVisit.v1", JSON.stringify({ loupist: { lastSeenAtMs: LAST_SEEN } }));
});
afterEach(async () => {
  vi.useRealTimers();
  localStorage.clear();
  await act(async () => { await i18n.changeLanguage("en"); });
});

describe("Morning message event strip", () => {
  it("lists what is coming up above the lead, one line each, and opens each where it lives", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const { handlers, openAsset, source } = renderMessage();
    const message = await screen.findByRole("article", { name: "Message from Quality Compounder" });
    const strip = await within(message).findByRole("region", { name: "Coming up" });
    expect(source.upcomingEvents).toHaveBeenCalledWith({ loupistId: "loupist" });
    expect(within(strip).getAllByRole("button").map((row) => row.getAttribute("aria-label"))).toEqual([
      "Mon, Nov 2 · Morrow Components · Results (expected)",
      "Tue, Nov 3 · To check: Gross margin stays above 55%",
      "Fri, Nov 6 · Hanul Precision · Results due (filing deadline)",
      "Mon, Nov 9 · Bluehaven Memory · Dividend record date",
    ]);
    // The strip leads; the lead and its bullets follow.
    const lead = await within(message).findByText("One thing since Thursday.");
    expect(strip.compareDocumentPosition(lead) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // The check in the strip is not repeated as a bullet nor counted as news; the overdue one stays.
    expect(within(message).getByText("Overdue").parentElement).toHaveTextContent("Overdue since Oct 29: Inventory days below 50");
    expect(within(message).queryByText(/To check by Nov 3/)).toBeNull();

    await user.click(within(strip).getByRole("button", { name: /Morrow Components · Results/ }));
    expect(openAsset).toHaveBeenCalledWith({ loupistId: "loupist", assetId: morrow.assetId, name: "Morrow Components", section: "results", eventId: "ern_00000000000000b1" });
    await user.click(within(strip).getByRole("button", { name: /Dividend record date/ }));
    expect(openAsset).toHaveBeenLastCalledWith({ loupistId: "loupist", assetId: "ast_00000000000000a3", name: "Bluehaven Memory" });
    await user.click(within(strip).getByRole("button", { name: /To check/ }));
    expect(handlers.onOpenJournal).toHaveBeenCalledWith("j2", "Gross margin stays above 55%");
  });

  it("names a Korean-listed asset by its Korean name and keeps the deadline wording", async () => {
    await act(async () => { await i18n.changeLanguage("ko"); });
    renderMessage();
    const strip = await screen.findByRole("region", { name: "다가오는 일정" });
    const deadline = within(strip).getByRole("button", { name: /실적 공시 마감/ });
    expect(deadline).toHaveTextContent("한울정밀");
    expect(deadline).toHaveTextContent("실적 공시 마감 (법정 기한)");
    expect(within(strip).getByRole("button", { name: /배당 기준일/ })).toHaveTextContent("Bluehaven Memory");
  });

  it("shows no strip when nothing is ahead", async () => {
    const { source } = renderMessage(bridge([]));
    expect(await screen.findByText("2 things since Thursday.")).toBeVisible();
    expect(source.upcomingEvents).toHaveBeenCalled();
    expect(screen.queryByRole("region", { name: "Coming up" })).toBeNull();
  });

  it("leaves the strip out of an as-of chat, where the check stays a bullet", async () => {
    const { source } = renderMessage(bridge(), { asOf: "2026-06-30" });
    const message = await screen.findByRole("article", { name: "Message from Quality Compounder" });
    expect(await within(message).findByText("2 things since Thursday.")).toBeVisible();
    expect(within(message).getByText("To check by Nov 3: Gross margin stays above 55%")).toBeVisible();
    expect(within(message).queryByRole("region", { name: "Coming up" })).toBeNull();
    expect(source.upcomingEvents).not.toHaveBeenCalled();
  });

  it("never asks Rust for the strip when a topic Chat offers only questions", async () => {
    const { source } = renderMessage(bridge(), { mode: "questions" });
    expect(await screen.findByRole("region", { name: "You could ask" })).toBeVisible();
    expect(screen.queryByRole("region", { name: "Coming up" })).toBeNull();
    expect(source.upcomingEvents).not.toHaveBeenCalled();
  });
});
