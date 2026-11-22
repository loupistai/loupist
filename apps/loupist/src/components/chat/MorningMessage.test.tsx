import { act, renderHook, screen, within } from "@testing-library/react";
import { renderWithOpen } from "../../test/open";
import userEvent from "@testing-library/user-event";
import i18n from "../../i18n";
import type { GrowthEvent, JournalRecord, KnowledgeMap } from "../../contracts/knowledge";
import type { InboxItem } from "../../contracts/scheduled";
import type { LoupistBridge } from "../../types";
import { MorningMessage } from "./MorningMessage";
import { mockAssetId } from "../../bridge/mock/assets";
import { markActive, useLoupistVisit } from "../../app/loupistVisit";

const HOUR = 3_600_000;
// Saturday 9:00; the last visit ended Thursday.
const NOW = new Date(2026, 9, 31, 9, 0).getTime();
const LAST_SEEN = NOW - 2 * 24 * HOUR;

const map = { nodes: [
  { id: "hanul", kind: "entity", label: "Hanul Precision", localNames: ["한울정밀"], entityKind: "Organization", degree: 2, watched: true, recordId: "world-1" },
  { id: "morrow", kind: "entity", label: "Morrow Components", entityKind: "Organization", degree: 1, watched: false, recordId: "world-1" },
], edges: [{ id: "c1", kind: "claim", source: "morrow", target: "hanul", relation: "supplies" }] } as unknown as KnowledgeMap;
const growth = (kind: GrowthEvent["kind"], subjectId: string): GrowthEvent => ({
  eventId: `${kind}-${subjectId}`, loupistId: "loupist", atMs: LAST_SEEN + HOUR, kind, recordId: "world-1", subjectId,
  originChatId: null, originMessageId: null, actor: "loupist", summary: "", beforeSummary: null, afterSummary: null,
});
const briefing = { runId: "run-1", taskId: "task", kind: "briefing", title: "Morning briefing", status: "succeeded",
  summary: "Hanul Precision's qualification costs rose. The margin settled.", startedAtMs: NOW - HOUR, finishedAtMs: NOW - HOUR, readAtMs: null } as InboxItem;
const journal = [{ journalId: "j1", judgment: "Inventory falls", archived: false,
  check: { claim: "Inventory days below 50", metric: "inventory days", direction: "below", threshold: 50, byDate: "2026-10-29" }, originChatId: null }] as JournalRecord[];

function bridge(overrides: Partial<LoupistBridge> = {}) {
  return {
    knowledgeMap: vi.fn(async () => map),
    knowledgeGrowthEvents: vi.fn(async () => ({ events: [growth("entity_added", "hanul"), growth("entity_added", "morrow"), growth("claim_added", "c1")], nextCursor: null })),
    inboxList: vi.fn(async () => ({ items: [briefing], nextCursor: null })),
    journalList: vi.fn(async () => journal),
    assetsList: vi.fn(async () => [{ assetId: mockAssetId("NVIDIA"), name: "NVIDIA", reference: null, followedAtMs: 0, createdAtMs: 0 }]),
    ...overrides,
  } as unknown as LoupistBridge;
}

function renderNews(source = bridge(), lastWroteAtMs?: number) {
  const handlers = { onAsk: vi.fn() };
  const view = renderWithOpen(<MorningMessage bridge={source} loupistId="loupist" loupistName="Quality Compounder" lastWroteAtMs={lastWroteAtMs} {...handlers} />);
  return { handlers, source, ...view };
}

const seen = (lastSeenAtMs: number) => localStorage.setItem("loupist.loupistVisit.v1", JSON.stringify({ loupist: { lastSeenAtMs } }));

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.setSystemTime(NOW);
  seen(LAST_SEEN);
});
afterEach(async () => {
  vi.useRealTimers();
  localStorage.clear();
  await act(async () => { await i18n.changeLanguage("en"); });
});

describe("Bot's news", () => {
  it("tells only what the person follows, counts every line it shows, and opens each where it lives", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const { source, open } = renderNews();
    const message = await screen.findByRole("article", { name: "Message from Quality Compounder" });
    // The briefing and the overdue check; prices are on the tiles and asset pages, not here.
    expect(within(message).getByText("2 things since Thursday.")).toBeVisible();
    expect(within(message).getAllByRole("listitem").filter((item) => item.closest(".morning-card"))).toHaveLength(2);
    // The Map shows its own growth: no counts, no names from it, no questions built from it.
    expect(source.knowledgeGrowthEvents).not.toHaveBeenCalled();
    expect(message).not.toHaveTextContent(/in World|relationship|Morrow Components/);
    // A question about the overdue check says so, as the bullet above it does.
    expect(within(screen.getByRole("region", { name: "You could ask" })).getAllByRole("button").map((button) => button.textContent)).toEqual([
      "Is my check on inventory days still on track?overdue since Oct 29",
      "Give me a quick briefing on NVIDIAyou follow it",
      "What should I know this week?",
    ]);
    expect(within(message).getByText(/^Morning briefing \(Oct 31, .+\): Hanul Precision's qualification costs rose\.$/)).toBeVisible();
    expect(within(message).getByText("Overdue").parentElement).toHaveTextContent("Overdue since Oct 29: Inventory days below 50");

    await user.click(within(message).getByRole("button", { name: "Open" }));
    expect(open).toHaveBeenLastCalledWith({ kind: "briefing", runId: "run-1" });
    // Review says which check it opens: the line beside it describes it.
    const review = within(message).getByRole("button", { name: "Review this check" });
    expect(review).toHaveAccessibleDescription("Overdue since Oct 29: Inventory days below 50");
    await user.click(review);
    expect(open).toHaveBeenLastCalledWith({ kind: "memory", location: { loupistId: "loupist", surface: "note", objectId: "j1", selection: { kind: "journal", journalId: "j1" } }, title: "Inventory days below 50" });
  });

  it("never appears while the person is using the app", async () => {
    // They wrote 33 minutes ago, after a visit that ended two days ago.
    const { source, unmount } = renderNews(bridge(), NOW - 33 * 60_000);
    await act(async () => { vi.advanceTimersByTime(50); });
    expect(screen.queryByRole("article")).toBeNull();
    expect(source.assetsList).not.toHaveBeenCalled();
    unmount();
    // A visit that ended three hours ago this morning is not an absence either.
    seen(NOW - 3 * HOUR);
    const again = renderNews();
    await act(async () => { vi.advanceTimersByTime(50); });
    expect(screen.queryByRole("article")).toBeNull();
    again.unmount();
    // Four hours away is.
    seen(NOW - 4 * HOUR);
    renderNews();
    expect(await screen.findByText("2 things since 5:00 AM.")).toBeVisible();
  });

  it("appears on the first open of a new day after an hour away", async () => {
    vi.setSystemTime(new Date(2026, 9, 31, 0, 30).getTime());
    seen(new Date(2026, 9, 30, 23, 0).getTime());
    renderNews(bridge({ journalList: vi.fn(async () => []) }));
    expect(await screen.findByText("One thing since Friday.")).toBeVisible();
  });

  it("says nothing when nothing the person follows has news, and leaves out a section that fails", async () => {
    const quiet = renderNews(bridge({ inboxList: vi.fn(async () => ({ items: [], nextCursor: null })), journalList: vi.fn(async () => []) }));
    await act(async () => { vi.advanceTimersByTime(50); });
    expect(screen.queryByRole("article")).toBeNull();
    quiet.unmount();
    renderNews(bridge({ journalList: vi.fn(async () => { throw new Error("offline"); }) }));
    const message = await screen.findByRole("article", { name: "Message from Quality Compounder" });
    expect(within(message).getByText("One thing since Thursday.")).toBeVisible();
    expect(within(message).queryByRole("button", { name: "Review this check" })).not.toBeInTheDocument();
  });

  describe("during a visit", () => {
    const MINUTE = 60_000;
    let visit: { unmount: () => void } | null = null;
    /** Opens the Loupist at `openedAtMs` as the app does, then returns the clock to now. */
    const openVisit = (openedAtMs: number) => {
      vi.setSystemTime(openedAtMs);
      visit = renderHook(() => useLoupistVisit("loupist", true));
      vi.setSystemTime(NOW);
    };
    afterEach(() => { visit?.unmount(); visit = null; });

    it("says nothing in a visit opened five hours ago with a session message ten minutes ago", async () => {
      openVisit(NOW - 5 * HOUR);
      vi.setSystemTime(NOW - 10 * MINUTE);
      act(() => markActive("loupist"));
      vi.setSystemTime(NOW);
      const { source } = renderNews();
      await act(async () => { vi.advanceTimersByTime(50); });
      expect(screen.queryByRole("article")).toBeNull();
      expect(source.assetsList).not.toHaveBeenCalled();
    });

    it("shows the news once in a visit that opened now after five hours away, and keeps it when the person then writes", async () => {
      seen(NOW - 5 * HOUR);
      openVisit(NOW);
      const { rerender, source } = renderNews();
      expect(await screen.findByText("2 things since 4:00 AM.")).toBeVisible();
      rerender(<MorningMessage bridge={source} loupistId="loupist" loupistName="Quality Compounder" onAsk={vi.fn()} />);
      expect(screen.getAllByRole("article")).toHaveLength(1);
      // Answering the news is a send: what is already on screen stays.
      act(() => markActive("loupist"));
      expect(screen.getByText("2 things since 4:00 AM.")).toBeVisible();
    });

    it("says nothing when the news finishes loading after the person sent a message", async () => {
      openVisit(NOW);
      let release = () => {};
      const held = new Promise<void>((resolve) => { release = resolve; });
      renderNews(bridge({ inboxList: vi.fn(async () => { await held; return { items: [briefing], nextCursor: null }; }) }));
      await act(async () => { vi.advanceTimersByTime(50); });
      expect(screen.queryByRole("article")).toBeNull();
      act(() => markActive("loupist"));
      await act(async () => { release(); vi.advanceTimersByTime(50); });
      expect(screen.queryByRole("article")).toBeNull();
    });
  });

  it("speaks Korean, and says one thing when one line is shown", async () => {
    await act(async () => { await i18n.changeLanguage("ko"); });
    renderNews(bridge({ inboxList: vi.fn(async () => ({ items: [], nextCursor: null })) }));
    expect(await screen.findByText("목요일 이후 한 가지입니다.")).toBeVisible();
  });
});

describe("A research session's questions", () => {
  function renderQuestions(source = bridge(), template?: "semiconductors" | "custom") {
    const handlers = { onAsk: vi.fn() };
    return { handlers, ...renderWithOpen(<MorningMessage bridge={source} loupistId="loupist" loupistName="Quality Compounder" mode="questions" template={template} {...handlers} />) };
  }

  it("offers three questions from what changed that go into the composer", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const { handlers } = renderQuestions();
    const ask = await screen.findByRole("region", { name: "You could ask" });
    const questions = within(ask).getAllByRole("button");
    expect(questions.map((button) => button.textContent)).toEqual([
      "What changed for Hanul Precision since Thursday?new in World",
      "Why is Morrow Components linked to Hanul Precision?new relationship",
      "Is my check on inventory days still on track?overdue since Oct 29",
    ]);
    expect(questions[0]).toHaveAccessibleName("What changed for Hanul Precision since Thursday?");
    expect(questions[0]).toHaveAccessibleDescription("new in World");
    await user.click(questions[1]!);
    expect(handlers.onAsk).toHaveBeenCalledWith("Why is Morrow Components linked to Hanul Precision?");
  });

  it("asks general questions on a first visit, and about a preset's subject but not a free-text focus", async () => {
    localStorage.clear();
    const empty = bridge({ knowledgeMap: vi.fn(async () => ({ ...map, nodes: [], edges: [] })), knowledgeGrowthEvents: vi.fn(async () => ({ events: [], nextCursor: null })),
      journalList: vi.fn(async () => []), assetsList: vi.fn(async () => []) });
    const first = renderQuestions(empty);
    expect(within(await screen.findByRole("region", { name: "You could ask" })).getAllByRole("button").map((button) => button.textContent)).toEqual([
      "What should I know this week?", "What happened in the markets this week?", "What risks should I keep an eye on?",
    ]);
    first.unmount();
    const preset = renderQuestions(empty, "semiconductors");
    expect(await screen.findByText("What matters in semiconductors this week?")).toBeVisible();
    preset.unmount();
    renderQuestions(empty, "custom");
    await screen.findByRole("region", { name: "You could ask" });
    expect(screen.queryByText(/^What matters in /)).toBeNull();
  });
});
