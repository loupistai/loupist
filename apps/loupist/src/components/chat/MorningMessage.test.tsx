import { act, screen, within } from "@testing-library/react";
import { openSpy, renderWithOpen } from "../../test/open";
import userEvent from "@testing-library/user-event";
import i18n from "../../i18n";
import type { GrowthEvent, JournalRecord, KnowledgeMap } from "../../contracts/knowledge";
import type { InboxItem } from "../../contracts/scheduled";
import type { LoupistBridge } from "../../types";
import { MorningMessage } from "./MorningMessage";

const HOUR = 3_600_000;
const NOW = new Date(2026, 9, 31, 9, 0).getTime();
const LAST_SEEN = NOW - 2 * 24 * HOUR;

const map = { nodes: [
  { id: "hanul", kind: "entity", label: "Hanul Precision", localNames: ["한울정밀"], entityKind: "Organization", degree: 2, watched: true, held: false, recordId: "world-1" },
  { id: "morrow", kind: "entity", label: "Morrow Components", entityKind: "Organization", degree: 1, watched: false, held: false, recordId: "world-1" },
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
    watchlistList: vi.fn(async () => []),
    ...overrides,
  } as unknown as LoupistBridge;
}

function renderMessage(source = bridge()) {
  const handlers = { onAsk: vi.fn() };
  const { open } = renderWithOpen(<MorningMessage bridge={source} loupistId="loupist" loupistName="Quality Compounder" {...handlers} />);
  return { handlers, source, open };
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

describe("Morning message", () => {
  it("tells what changed since the last visit and opens each item where it lives", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const { source, open } = renderMessage();
    const message = await screen.findByRole("article", { name: "Message from Quality Compounder" });
    expect(await within(message).findByText("3 things since Thursday.")).toBeVisible();
    expect(source.knowledgeGrowthEvents).toHaveBeenCalledWith(expect.objectContaining({ loupistId: "loupist", sinceMs: LAST_SEEN + 1 }));
    expect(within(message).getByText("+2 on the map and 1 relationship:")).toBeVisible();
    expect(within(message).getByText(/^Morning briefing \(Oct 31, .+\): Hanul Precision's qualification costs rose\.$/)).toBeVisible();
    const overdue = within(message).getByText("Overdue");
    expect(overdue).toHaveClass("morning-late");
    expect(overdue.parentElement).toHaveTextContent("Overdue since Oct 29: Inventory days below 50");

    await user.click(within(message).getByRole("button", { name: "Hanul Precision" }));
    expect(open).toHaveBeenLastCalledWith({ kind: "page", location: { loupistId: "loupist", surface: "world", recordId: "world-1", objectId: "hanul", selection: { kind: "node", id: "hanul" } } });
    await user.click(within(message).getByRole("button", { name: "See all in Map" }));
    expect(open).toHaveBeenLastCalledWith({ kind: "page", location: { loupistId: "loupist", surface: "world", view: { followCurrent: true, whatsNew: true } } });
    await user.click(within(message).getByRole("button", { name: "Open" }));
    expect(open).toHaveBeenLastCalledWith({ kind: "briefing", runId: "run-1" });
    await user.click(within(message).getByRole("button", { name: "Review this check" }));
    expect(open).toHaveBeenLastCalledWith({ kind: "memory", location: { loupistId: "loupist", surface: "note", objectId: "j1", selection: { kind: "journal", journalId: "j1" } }, title: "Inventory days below 50" });
  });

  it("offers three questions that go into the composer", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const { handlers } = renderMessage();
    const ask = await screen.findByRole("region", { name: "You could ask" });
    const questions = within(ask).getAllByRole("button");
    expect(questions.map((button) => button.textContent)).toEqual([
      "What changed for Hanul Precision since Thursday?new on the map",
      "Why is Morrow Components linked to Hanul Precision?new relationship",
      "Is my check on inventory days still on track?To check by Oct 29",
    ]);
    expect(questions[0]).toHaveAccessibleName("What changed for Hanul Precision since Thursday?");
    expect(questions[0]).toHaveAccessibleDescription("new on the map");
    await user.click(questions[1]!);
    expect(handlers.onAsk).toHaveBeenCalledWith("Why is Morrow Components linked to Hanul Precision?");
  });

  it("leaves out a section that fails and keeps the rest", async () => {
    renderMessage(bridge({ journalList: vi.fn(async () => { throw new Error("offline"); }), inboxList: vi.fn(async () => { throw new Error("offline"); }) }));
    const message = await screen.findByRole("article", { name: "Message from Quality Compounder" });
    expect(await within(message).findByText("One thing since Thursday.")).toBeVisible();
    expect(within(message).queryByRole("button", { name: "Review this check" })).not.toBeInTheDocument();
    expect(within(message).queryByRole("button", { name: "Open" })).not.toBeInTheDocument();
    expect(within(message).getByRole("button", { name: "See all in Map" })).toBeVisible();
  });

  it("shows a placeholder line only for a section that is still loading after 400 ms", async () => {
    let finish: (value: JournalRecord[]) => void = () => {};
    renderMessage(bridge({ journalList: vi.fn(() => new Promise<JournalRecord[]>((resolve) => { finish = resolve; })) }));
    const message = await screen.findByRole("article", { name: "Message from Quality Compounder" });
    expect(message).toHaveAttribute("aria-busy", "true");
    expect(message.querySelector(".morning-skeleton")).toBeNull();
    await act(async () => { vi.advanceTimersByTime(450); });
    expect(message.querySelectorAll(".morning-skeleton").length).toBeGreaterThan(0);
    await act(async () => { finish(journal); });
    expect(await within(message).findByText("3 things since Thursday.")).toBeVisible();
    expect(message.querySelector(".morning-skeleton")).toBeNull();
    expect(message).toHaveAttribute("aria-busy", "false");
  });

  it("greets a first visit and an empty Loupist", async () => {
    localStorage.clear();
    renderMessage(bridge({ knowledgeMap: vi.fn(async () => ({ ...map, nodes: [], edges: [] })), knowledgeGrowthEvents: vi.fn(async () => ({ events: [], nextCursor: null })),
      inboxList: vi.fn(async () => ({ items: [], nextCursor: null })), journalList: vi.fn(async () => []) }));
    expect(await screen.findByText("What are you researching?")).toBeVisible();
    expect(within(screen.getByRole("region", { name: "You could ask" })).getAllByRole("button").map((button) => button.textContent)).toEqual([
      "What should I know this week?", "What happened in the markets this week?", "What risks should I keep an eye on?",
    ]);
  });

  it("names the connections a class template works best with on its first message only while they are missing", async () => {
    const handlers = { onAsk: vi.fn() };
    const open = openSpy();
    const render = (ui: React.ReactElement) => renderWithOpen(ui, open);
    const missing = bridge({ loupistCapabilityList: vi.fn(async () => [{ entry_id: "eia.energy", enabled: false, granted_permissions: [], available: false }]) });
    // A later visit says nothing and does not ask.
    const later = render(<MorningMessage bridge={missing} loupistId="loupist" loupistName="Commodity Loupist" template="commodities" {...handlers} />);
    expect(await screen.findByText("3 things since Thursday.")).toBeVisible();
    expect(screen.queryByText(/Works best with/)).not.toBeInTheDocument();
    expect(missing.loupistCapabilityList).not.toHaveBeenCalled();
    later.unmount();
    // The first visit of a Commodities Loupist without EIA names it, with a way to Connections.
    localStorage.clear();
    const first = render(<MorningMessage bridge={missing} loupistId="loupist" loupistName="Commodity Loupist" template="commodities" {...handlers} />);
    const line = await screen.findByText(/^Works best with EIA/);
    expect(line).toHaveTextContent("Works best with EIA · Open Connections");
    await userEvent.click(within(line).getByRole("button", { name: "Open Connections" }));
    expect(open).toHaveBeenCalledExactlyOnceWith({ kind: "settings", section: "connections" });
    await act(async () => { await i18n.changeLanguage("ko"); });
    expect(await screen.findByText(/EIA와 함께 쓰면 더 좋아요/)).toHaveTextContent("EIA와 함께 쓰면 더 좋아요 · 연결 열기");
    first.unmount();
    await act(async () => { await i18n.changeLanguage("en"); });
    // Connected, or a template without recommendations, says nothing.
    const connected = bridge({ loupistCapabilityList: vi.fn(async () => [{ entry_id: "eia.energy", enabled: true, granted_permissions: [], available: true }]) });
    const done = render(<MorningMessage bridge={connected} loupistId="loupist" loupistName="Commodity Loupist" template="commodities" {...handlers} />);
    await screen.findByRole("region", { name: "You could ask" });
    expect(connected.loupistCapabilityList).toHaveBeenCalled();
    expect(screen.queryByText(/Works best with/)).not.toBeInTheDocument();
    done.unmount();
    const custom = bridge({ loupistCapabilityList: vi.fn(async () => []) });
    render(<MorningMessage bridge={custom} loupistId="loupist" loupistName="Desk" template="custom" {...handlers} />);
    await screen.findByRole("region", { name: "You could ask" });
    expect(custom.loupistCapabilityList).not.toHaveBeenCalled();
    expect(screen.queryByText(/Works best with/)).not.toBeInTheDocument();
  });

  it("speaks Korean with local names", async () => {
    await act(async () => { await i18n.changeLanguage("ko"); });
    renderMessage(bridge({ journalList: vi.fn(async () => []), inboxList: vi.fn(async () => ({ items: [], nextCursor: null })) }));
    expect(await screen.findByText("목요일 이후 한 가지예요.")).toBeVisible();
    expect(screen.getByRole("button", { name: "한울정밀" })).toBeVisible();
    expect(within(screen.getByRole("region", { name: "이렇게 물어볼 수 있어요" })).getAllByRole("button")[0]).toHaveTextContent(/^목요일 이후 한울정밀에 무슨 변화가 있었어\?/);
  });
  it("teaches three first moves when nothing is followed and no briefing is on, filling the composer without sending", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const empty = bridge({
      inboxList: vi.fn(async () => ({ items: [], nextCursor: null })),
      briefingSettingsGet: vi.fn(async () => ({ enabled: false, schedule: null, task: null })),
    } as unknown as Partial<LoupistBridge>);
    const { handlers } = renderMessage(empty);
    const setup = await screen.findByRole("region", { name: "Get set up" });
    const chips = within(setup).getAllByRole("button");
    expect(chips.map((chip) => chip.textContent)).toEqual([
      "Follow AAPL and 005930",
      "Turn on a weekday morning briefing at 8",
      "Remember: check whether capacity costs are temporary before trusting margin recovery",
      "Import holdings (CSV)",
    ]);
    await user.click(chips[1]!);
    expect(handlers.onAsk).toHaveBeenCalledWith("Turn on a weekday morning briefing at 8");
  });

  it("leaves setup out once a briefing is on, and asks about a preset's subject but not a free-text focus", async () => {
    const on = bridge({
      knowledgeGrowthEvents: vi.fn(async () => ({ events: [], nextCursor: null })),
      inboxList: vi.fn(async () => ({ items: [], nextCursor: null })),
      journalList: vi.fn(async () => []),
      briefingSettingsGet: vi.fn(async () => ({ enabled: true, schedule: null, task: null })),
    } as unknown as Partial<LoupistBridge>);
    const handlers = { onAsk: vi.fn() };
    const preset = renderWithOpen(<MorningMessage bridge={on} loupistId="loupist" loupistName="Chip Loupist" template="semiconductors" {...handlers} />);
    await screen.findByRole("region", { name: "You could ask" });
    expect(screen.queryByRole("region", { name: "Get set up" })).toBeNull();
    expect(screen.getByText("What matters in semiconductors this week?")).toBeVisible();
    preset.unmount();
    renderWithOpen(<MorningMessage bridge={on} loupistId="loupist" loupistName="Desk" template="custom" {...handlers} />);
    await screen.findByRole("region", { name: "You could ask" });
    expect(screen.queryByText(/^What matters in /)).toBeNull();
  });
});
