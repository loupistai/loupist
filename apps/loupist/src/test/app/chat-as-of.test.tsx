import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { lookBackDraft, LookBackProvider } from "../../components/knowledge/lookBack";
import { JournalThenNow } from "../../components/knowledge/ResearchJournal";
import { AsOfNote } from "../../components/chat/AsOfNote";
import { ChatMessageCard } from "../../components/chat/ChatMessageCard";
import { configureTheme, PreviewBridge } from "../../preview/fixtures";
import { scenarios } from "../../preview/scenarios";
import type { JournalRecord } from "../../types";
import { openApp } from "../renderApp";

type User = ReturnType<typeof userEvent.setup>;
const composer = () => screen.getByRole("textbox", { name: "Message Loupist" });
const gate = () => {
  let open!: () => void;
  const promise = new Promise<void>((resolve) => { open = resolve; });
  return { promise, open };
};
const type = async (user: User, text: string) => {
  await user.click(composer());
  await user.type(composer(), text);
};

describe("as-of answers", () => {
  it("turns a typed /asof date into the first chip, sends it, and keeps it for follow-ups", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const native = bridge.chatSend.bind(bridge);
    const running = gate();
    const chatSend = vi.spyOn(bridge, "chatSend").mockImplementationOnce(async (...args) => { await running.promise; return native(...args); });
    await openApp(bridge);
    await type(user, "/asof 2026-03-03 ");
    const chip = screen.getByRole("button", { name: /As of Mar 3, 2026 · Change the as-of date/ });
    expect(chip).toHaveTextContent("As of Mar 3, 2026");
    expect(composer()).toHaveValue("");
    expect(composer()).toHaveAttribute("placeholder", "Ask what was known on Mar 3, 2026…");
    await type(user, "What did we know then?");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(chatSend).toHaveBeenCalledOnce());
    expect(chatSend.mock.calls[0][0]).toMatchObject({ prompt: "What did we know then?", as_of: "2026-03-03" });
    // The chip stays for the next message, and a queued message keeps its own date.
    expect(screen.getByRole("button", { name: /Change the as-of date/ })).toBeVisible();
    await type(user, "And the margin?");
    await user.click(screen.getByRole("button", { name: "Send next" }));
    const queue = screen.getByLabelText("Pending chat instructions");
    expect(within(queue).getAllByLabelText("As of March 3, 2026")[0]).toBeVisible();
    // Removing the chip changes only the next message.
    await user.click(screen.getByRole("button", { name: "Remove as-of date" }));
    expect(screen.queryByRole("button", { name: /Change the as-of date/ })).toBeNull();
    await act(async () => running.open());
    await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2));
    expect(chatSend.mock.calls[1][0]).toMatchObject({ prompt: "And the margin?", as_of: "2026-03-03" });
    // The request and its answer carry the date.
    await waitFor(() => expect(screen.getAllByLabelText("As of March 3, 2026").length).toBeGreaterThanOrEqual(3));
  });

  it("refuses a future date in the date panel and opens the panel from the / menu", async () => {
    const user = userEvent.setup();
    await openApp();
    await type(user, "/asof 2999-01-01 ");
    const panel = screen.getByRole("group", { name: "Look back to" });
    expect(within(panel).getByRole("alert")).toHaveTextContent("Pick a date on or before today.");
    expect(within(panel).getByRole("button", { name: "Set date" })).toBeDisabled();
    await user.click(within(panel).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("group", { name: "Look back to" })).toBeNull();
    await type(user, "/as");
    // A command whose name starts with what was typed ranks first, ahead of a match in another's description.
    expect(screen.getAllByRole("option")[0]).toHaveAccessibleName("/asof");
    await user.click(screen.getByRole("option", { name: "/asof" }));
    const dated = screen.getByRole("group", { name: "Look back to" });
    expect(within(dated).getByLabelText("Year")).toHaveFocus();
    fireEvent.change(within(dated).getByLabelText("Year"), { target: { value: "2026" } });
    fireEvent.change(within(dated).getByLabelText("Month"), { target: { value: "3" } });
    fireEvent.change(within(dated).getByLabelText("Day"), { target: { value: "3" } });
    await user.click(within(dated).getByRole("button", { name: "Set date" }));
    expect(screen.getByRole("button", { name: /As of Mar 3, 2026 · Change the as-of date/ })).toBeVisible();
  });

  it("renders a refused step as neutral and the note from the answer's report", () => {
    render(<ChatMessageCard loupistId="loupist-a" loupistName="Quality" onReadAttachment={vi.fn()}
      message={{ id: "answer", role: "assistant", status: "complete", content: "Done.", created_at: "2026-03-03T00:00:00Z", as_of: "2026-03-03", progress: { startedAtMs: 1, finishedAtMs: 50_000, items: [
        { id: "quote", kind: "tool", toolName: "kis_read", category: "finance", operation: "read", action: "Read market data", target: "990221", status: "skipped", startedAtMs: 1, finishedAtMs: 2, href: null },
      ] } }} />);
    // A refused lookup is not a failure: the summary says "skipped" and the details stay closed until asked.
    const summary = screen.getByRole("button", { name: /1 lookup skipped for Mar 3, 2026/ });
    expect(summary).not.toHaveTextContent(/failed/u);
    expect(summary).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(summary);
    const row = document.querySelector(".chat-progress-row.skipped")!;
    // The row names the source as a noun and leaves the date to the answer's tag; it leads with the skipped glyph.
    expect(row).toHaveTextContent("Can't look back to that day · Market data");
    expect(row.querySelector(".chat-progress-node")).toBe(within(row as HTMLElement).getByLabelText("Not available for this date").parentElement);
    render(<AsOfNote loupistName="Quality" report={{ cutoff: "2026-03-03", refused: [{ tool: "kis_read" }], empty: [{ tool: "krx_daily_quote", target: "990221" }], left_out: { facts: 12, sources: 2, notes: 0, methods: 1 } }} />);
    const note = screen.getByRole("region", { name: "Looked back to Mar 3, 2026." });
    expect(within(note).getAllByRole("listitem").map((item) => item.textContent)).toEqual([
      "Couldn't show as known then: market data.",
      "Nothing dated on or before that day in: daily prices for 990221.",
      "Left out what Quality learned later: 12 facts, 2 sources, 1 method.",
    ]);
    const { container } = render(<AsOfNote loupistName="Quality" report={{ cutoff: "2026-03-03", left_out: { facts: 0, sources: 0, notes: 0, methods: 0 } }} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("offers Look back from the Journal's Then · Now with the entry's local day", async () => {
    const user = userEvent.setup();
    const created = new Date(2026, 2, 5, 9).getTime();
    const entry = { journalId: "j1", researchId: "", loupistId: "l1", judgment: "Solsup's FY2025 revenue will beat ₩445bn.", reasoning: "", entities: [],
      check: { claim: "Revenue above ₩445bn", metric: "revenue", direction: "above", threshold: 445, byDate: "2026-03-31" },
      createdAtMs: created, updatedAtMs: created, archived: false, inputDigest: "", inputs: { sources: [], worlds: [], lenses: [], earlier: [] }, originChatId: null } as JournalRecord;
    const lookBack = vi.fn();
    render(<LookBackProvider value={lookBack}><JournalThenNow entry={entry} /></LookBackProvider>);
    expect(screen.getByText("No sources were saved with this entry.")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Look back with only what was known then" }));
    expect(lookBack).toHaveBeenCalledWith(expect.objectContaining({ journalId: "j1", createdAtMs: created }));
    expect(lookBackDraft(lookBack.mock.calls[0][0])).toEqual({
      title: "Looking back: Solsup's FY2025 revenue will beat ₩445bn…",
      asOf: "2026-03-05",
      pin: { kind: "journal", loupistId: "l1", objectId: "j1", label: "Solsup's FY2025 revenue will beat ₩445bn…" },
      prompt: "Using only what was known on Mar 5, 2026, how well supported was this judgment, and what was missing?",
    });
  });

  it("Look back opens a new chat with both chips and the question, and never sends", async () => {
    const scenario = scenarios.find((item) => item.id === "journal-lookback-draft")!;
    const bridge = new PreviewBridge({ delay_ms: 0 });
    await scenario.setup(bridge);
    await configureTheme(bridge, "light");
    const chatSend = vi.spyOn(bridge, "chatSend");
    const create = vi.spyOn(bridge, "chatCreate");
    const view = render(<App bridge={bridge} />);
    await scenario.enter(view.container);
    await waitFor(() => expect(scenario.landmark()).toBeVisible(), { timeout: 8000 });
    expect(create).toHaveBeenCalledWith(expect.objectContaining({ title: expect.stringMatching(/^Looking back: /) }));
    await waitFor(() => expect((composer() as HTMLTextAreaElement).value).toMatch(/Using only what was known on Mar 5, 2026/));
    expect(chatSend).not.toHaveBeenCalled();
    // The draft is the question: no generic starter questions compete with it.
    expect(document.querySelector(".morning-ask")).toBeNull();
  });

  it("words Then · Now like the app's checks, with amounts as answers print them", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 9, 28, 12));
    try {
      const created = new Date(2026, 2, 5, 9).getTime();
      const base = { researchId: "", loupistId: "l1", reasoning: "", entities: [], createdAtMs: created, updatedAtMs: created, archived: false, inputDigest: "", inputs: { sources: [], worlds: [], lenses: [], earlier: [] } };
      const open = { ...base, journalId: "open", judgment: "Margin stays above 9%.",
        check: { claim: "Operating margin above 9%", metric: "operating margin", direction: "above", threshold: 9, unit: "%", byDate: "2026-10-01" }, originChatId: null } as JournalRecord;
      const view = render(<JournalThenNow entry={open} />);
      // An open check was never checked: no "checked" date, and the overdue wording from the rest of the app.
      expect(screen.getByRole("heading", { name: "Now · not checked yet" })).toBeVisible();
      expect(screen.getByText("Overdue since Oct 1")).toHaveClass("journal-then-now-late");
      view.unmount();
      const reviewed = { ...base, journalId: "done", judgment: "Revenue will beat ₩445bn.", updatedAtMs: new Date(2026, 3, 2, 9).getTime(),
        check: { claim: "Revenue above ₩445bn", metric: "revenue", direction: "above", threshold: 445_000_000_000, unit: "KRW", byDate: "2026-03-31" },
        outcome: { observed: 452_312_000_000, observedAt: "2026-03-18", result: "came_true" }, originChatId: null } as JournalRecord;
      render(<JournalThenNow entry={reviewed} />);
      expect(screen.getByRole("heading", { name: "Now · checked Apr 2, 2026" })).toBeVisible();
      expect(screen.getByText("On Mar 18, 2026: revenue was ₩452.3bn → came true")).toBeVisible();
    } finally {
      vi.useRealTimers();
    }
  });
});
