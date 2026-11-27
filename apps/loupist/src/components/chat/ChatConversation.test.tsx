import { Chat } from "@ai-sdk/react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mockAssetId } from "../../bridge/mock/assets";
import { TauriChatTransport, toLoupistUIMessage, type LoupistUIMessage } from "../../chat/ai-sdk";
import { useLoupistChat } from "../../chat/useLoupistChat";
import type { ChatMessage } from "../../types";
import { ChatConversation } from "./ChatConversation";
import { SafeMessageResponse } from "./SafeMessageResponse";
import * as composerMentions from "../../chat/composerMentions";

/** A followed asset known by its name alone, as `assets_list` returns it. */
const followedRow = (assetId: string, name: string) => ({ assetId, name, reference: null, followedAtMs: 0, createdAtMs: 0 });

vi.mock("./SafeMessageResponse", () => ({
  SafeMessageResponse: vi.fn(({ text }: { text: string }) => <p>{text}</p>),
}));

it("updates the live reply without rendering saved answers again, and can edit any sent message (X18)", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const message = (id: string, role: ChatMessage["role"], content: string): ChatMessage => ({
    id, role, content, status: "complete", created_at: "2026-10-24T00:00:00Z",
  });
  const chat = new Chat<LoupistUIMessage>({
    transport: new TauriChatTransport(bridge),
    messages: [
      message("user-1", "user", "Previous question"),
      message("answer-1", "assistant", "Saved answer"),
      message("user-2", "user", "Current question"),
      { ...message("answer-2", "assistant", "Working"), status: "streaming" as const },
    ].map(toLoupistUIMessage),
  });
  const props = {
    loupistId: "loupist-a", loupistName: "Loupist", onSuggestion: vi.fn(), onEditUser: vi.fn(),
    onOpenMemory: vi.fn(), onOpenMemoryPage: vi.fn(), onOpenArtifact: vi.fn(),
    onOpenLink: vi.fn(), onReadAttachment: vi.fn(),
  };
  const setThreads = vi.fn();
  function Conversation() {
    const { messages, messageKeys } = useLoupistChat({ bridge, chat, loupistId: props.loupistId, activeThread: undefined, setThreads });
    return <ChatConversation {...props} messages={messages} messageKeys={messageKeys} />;
  }
  render(<Conversation />);
  expect(screen.getByText("Saved answer")).toBeVisible();
  const mentions = vi.spyOn(composerMentions, "knownComposerMentions");
  vi.mocked(SafeMessageResponse).mockClear();

  for (const text of ["Working with sources", "Finished answer"]) {
    act(() => {
      chat.messages = chat.messages.map((item) => item.id === "answer-2"
        ? toLoupistUIMessage({ ...message("answer-2", "assistant", text), status: text === "Finished answer" ? "complete" : "streaming" }) : item);
    });
    await waitFor(() => expect(screen.getByText(text)).toBeVisible());
  }
  expect(vi.mocked(SafeMessageResponse).mock.calls.filter(([props]) => props.text === "Saved answer")).toHaveLength(0);
  expect(mentions).not.toHaveBeenCalled();
  const edits = screen.getAllByRole("button", { name: "Edit message" });
  expect(edits).toHaveLength(2);
  await userEvent.setup().click(edits[1]!);
  expect(props.onEditUser).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ content: "Current question" }), 2);

  act(() => {
    chat.messages = chat.messages.map((item) => item.id === "answer-1"
      ? toLoupistUIMessage(message("answer-1", "assistant", "Refreshed saved answer")) : item);
  });
  await waitFor(() => expect(screen.getByText("Refreshed saved answer")).toBeVisible());
  expect(screen.queryByText("Saved answer")).not.toBeInTheDocument();
});

describe("New since you left (X5)", () => {
  const message = (id: string, role: ChatMessage["role"], at: string): ChatMessage => ({ id, role, content: id, status: "complete", created_at: at });
  const props = {
    loupistId: "loupist-a", chatId: "chat-away", loupistName: "Loupist", onSuggestion: vi.fn(),
    onOpenMemory: vi.fn(), onOpenMemoryPage: vi.fn(), onOpenArtifact: vi.fn(), onOpenLink: vi.fn(), onReadAttachment: vi.fn(),
  };
  const first = [message("q1", "user", "2026-10-28T08:59:00Z"), message("a1", "assistant", "2026-10-28T09:00:00Z")];
  const later = [...first, message("q2", "user", "2026-10-28T09:01:50Z"), message("a2", "assistant", "2026-10-28T09:05:00Z")];
  const divider = () => screen.queryByRole("separator", { name: "New since you left" });
  const setVisibility = (state: DocumentVisibilityState) => {
    vi.spyOn(document, "visibilityState", "get").mockReturnValue(state);
    act(() => { document.dispatchEvent(new Event("visibilitychange")); });
  };
  beforeEach(() => {
    window.localStorage.clear();
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(Date.parse("2026-10-28T09:01:00Z"));
  });
  afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

  it("does not mark an answer that finished while the window was visible but unfocused", () => {
    const view = render(<ChatConversation {...props} messages={first} messageKeys={first.map((item) => item.id)} />);
    // The user switches to another app; the window stays visible with the chat on screen.
    vi.setSystemTime(Date.parse("2026-10-28T09:02:00Z"));
    act(() => { window.dispatchEvent(new Event("blur")); });
    expect(document.visibilityState).toBe("visible");
    view.rerender(<ChatConversation {...props} messages={later} messageKeys={later.map((item) => item.id)} />);
    vi.setSystemTime(Date.parse("2026-10-28T09:06:00Z"));
    act(() => { window.dispatchEvent(new Event("focus")); });
    expect(divider()).toBeNull();
  });

  it("marks an answer that finished while the window was hidden", () => {
    const view = render(<ChatConversation {...props} messages={first} messageKeys={first.map((item) => item.id)} />);
    vi.setSystemTime(Date.parse("2026-10-28T09:02:00Z"));
    setVisibility("hidden");
    view.rerender(<ChatConversation {...props} messages={later} messageKeys={later.map((item) => item.id)} />);
    expect(divider()).toBeNull();
    vi.setSystemTime(Date.parse("2026-10-28T09:06:00Z"));
    setVisibility("visible");
    expect(divider()?.nextElementSibling).toHaveAttribute("data-message-id", "a2");
  });

  it("marks an answer that finished while another conversation was showing", () => {
    const view = render(<ChatConversation {...props} messages={first} messageKeys={first.map((item) => item.id)} />);
    vi.setSystemTime(Date.parse("2026-10-28T09:02:00Z"));
    view.rerender(<ChatConversation {...props} active={false} messages={first} messageKeys={first.map((item) => item.id)} />);
    view.rerender(<ChatConversation {...props} active={false} messages={later} messageKeys={later.map((item) => item.id)} />);
    vi.setSystemTime(Date.parse("2026-10-28T09:06:00Z"));
    view.rerender(<ChatConversation {...props} active messages={later} messageKeys={later.map((item) => item.id)} />);
    expect(divider()?.nextElementSibling).toHaveAttribute("data-message-id", "a2");
  });
});

describe("Bot greeting", () => {
  const post: ChatMessage = { id: "host-1", role: "assistant", content: "Your World now also tracks suppliers.", status: "complete", created_at: "2026-10-24T00:00:00Z" };
  const render_ = (messages: ChatMessage[]) => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chat = new Chat<LoupistUIMessage>({ transport: new TauriChatTransport(bridge), messages: messages.map(toLoupistUIMessage) });
    function Conversation() {
      const { messages: list, messageKeys } = useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread: undefined, setThreads: vi.fn() });
      return <ChatConversation loupistId="loupist-quality" loupistName="Loupist" bridge={bridge} home messages={list} messageKeys={messageKeys} onSuggestion={vi.fn()} onEditUser={vi.fn()} onReadAttachment={vi.fn()} />;
    }
    return render(<Conversation />);
  };
  const setupBridge = () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    let followed: Awaited<ReturnType<MockLoupistBridge["assetsList"]>> = [];
    vi.spyOn(bridge, "assetsList").mockImplementation(async () => followed);
    vi.spyOn(bridge, "principleList").mockResolvedValue([]);
    vi.spyOn(bridge, "briefingSettingsGet").mockResolvedValue({ enabled: false, schedule: null, task: null } as unknown as Awaited<ReturnType<MockLoupistBridge["briefingSettingsGet"]>>);
    return { bridge, follow: (next: typeof followed) => { followed = next; } };
  };

  it("greets beside a host post until the user writes", async () => {
    render_([post]);
    expect(screen.getByText("Your World now also tracks suppliers.")).toBeVisible();
    // Nothing is followed yet, so the cards are first steps.
    expect(await screen.findByRole("region", { name: "Start here" })).toBeVisible();
  });

  it("is gone once the user has written, and leaves no setup steps under the conversation", async () => {
    const { bridge } = setupBridge();
    const sent: ChatMessage[] = [{ ...post, id: "user-1", role: "user", content: "Follow a few companies for me." }];
    render(<ChatConversation loupistId="loupist-quality" loupistName="Loupist" bridge={bridge} home messages={sent} messageKeys={["user-1"]} onSuggestion={vi.fn()} onReadAttachment={vi.fn()} />);
    expect(screen.getByText("Follow a few companies for me.")).toBeVisible();
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
    expect(screen.queryByRole("region", { name: "Loupist greets you" })).toBeNull();
    expect(document.querySelector("[data-morning-setup], [data-ask]")).toBeNull();
  });

  it("offers first moves to a new user, fills the composer, and drops Follow once an asset is followed elsewhere", async () => {
    const { bridge, follow } = setupBridge();
    const onSuggestion = vi.fn();
    const props = { loupistId: "loupist-quality", loupistName: "Loupist", bridge, home: true, messages: [], messageKeys: [], onSuggestion, onOpenFollowing: vi.fn(), onReadAttachment: vi.fn() };
    const view = render(<ChatConversation {...props} />);
    const greeting = await screen.findByRole("region", { name: "Loupist greets you" });
    await waitFor(() => expect(greeting).toHaveAttribute("aria-busy", "false"));
    expect(greeting.querySelector('[data-ask="follow"]')).toHaveTextContent("Add assets to follow");
    // Following carries no ownership, so no first move imports holdings.
    expect(greeting.querySelector('[data-ask="holdings"]')).toBeNull();
    expect(greeting).not.toHaveTextContent(/holdings/i);
    expect(screen.getByRole("region", { name: "Start here" })).toBeVisible();
    // The greeting names the date; the page header already names the Loupist.
    expect(greeting.querySelector(".bot-greeting-top small")).not.toHaveTextContent("Loupist");
    // The card is named as a topic; the composer gets the user's own request.
    await userEvent.click(greeting.querySelector<HTMLButtonElement>('[data-ask="markets"]')!);
    expect(onSuggestion).toHaveBeenCalledWith("What matters in markets this week? List the dates and figures to watch.");
    // The user follows Apple on Following, then comes back to Bot.
    view.rerender(<ChatConversation {...props} active={false} />);
    follow([followedRow(mockAssetId("Apple"), "Apple")]);
    view.rerender(<ChatConversation {...props} active />);
    await waitFor(() => expect(document.querySelector('[data-ask="status"]')).toHaveTextContent("Where Apple stands"));
    expect(document.querySelector('[data-ask="follow"]')).toBeNull();
    expect(screen.getByRole("region", { name: "Suggested questions" })).toBeVisible();
    await userEvent.click(document.querySelector<HTMLButtonElement>('[data-ask="status"]')!);
    expect(onSuggestion).toHaveBeenLastCalledWith("Where does Apple stand? Give me the price, results and news in brief.");
  });

  it("shows each followed asset's stored price as its replay stated it, with its date and provider, and no move it was not given", async () => {
    const { bridge, follow } = setupBridge();
    const nvidia = mockAssetId("NVIDIA"), apple = mockAssetId("Apple"), harbor = mockAssetId("Harbor Fund");
    follow([followedRow(nvidia, "NVIDIA"), followedRow(apple, "Apple"), followedRow(harbor, "Harbor Fund")]);
    vi.spyOn(bridge, "watchlistQuotes").mockResolvedValue([
      { assetId: nvidia, price: { value: 162.85, date: "2026-11-13", currency: "USD", currencySource: "result", provider: "yfinance", sourceUrl: null, dataset: { dataset_id: "a", revision: 1, digest: "test" } } },
      // A time as the provider stated it, and no currency: the price prints without one and says so.
      { assetId: apple, price: { value: 268.47, date: "2026-11-14T14:30:00Z", currency: null, currencySource: "none", provider: "fmp", sourceUrl: null, dataset: { dataset_id: "b", revision: 1, digest: "test" } } },
    ]);
    render(<ChatConversation loupistId="loupist-quality" loupistName="Loupist" bridge={bridge} home messages={[]} messageKeys={[]} onSuggestion={vi.fn()} onReadAttachment={vi.fn()} />);
    const greeting = await screen.findByRole("region", { name: "Loupist greets you" });
    await waitFor(() => expect(greeting.querySelectorAll(".bot-tile")).toHaveLength(2));
    const tiles = [...greeting.querySelectorAll(".bot-tile")];
    expect(tiles[0]).toHaveTextContent("NVIDIA");
    expect(tiles[0]!.querySelector(".bot-tile-foot")).toHaveTextContent("As of Nov 13 · Yahoo Finance");
    expect(tiles[1]!.querySelector(".bot-tile-foot")).toHaveTextContent(/^As of Nov 14 \d{1,2}:\d{2} [AP]M · Financial Modeling Prep$/u);
    expect(tiles[1]!.querySelector(".bot-tile-price")).not.toHaveTextContent("$");
    expect(tiles[1]!.querySelector("[data-price-currency=unknown]")).toHaveTextContent("Currency unknown");
    // No close or session wording, no change it was not given, and no news bubble built from prices.
    expect(greeting).not.toHaveTextContent(/close|fell|rose|%/iu);
    expect(greeting.querySelector(".bot-bubble")).toBeNull();
    expect(screen.getByRole("region", { name: "Following" })).toBeVisible();
  });

  it("reads the list and its prices again when the host says assets changed", async () => {
    const { bridge, follow } = setupBridge();
    const nvidia = mockAssetId("NVIDIA");
    follow([followedRow(nvidia, "NVIDIA")]);
    const quotes = vi.spyOn(bridge, "watchlistQuotes").mockResolvedValue([]);
    render(<ChatConversation loupistId="loupist-quality" loupistName="Loupist" bridge={bridge} home messages={[]} messageKeys={[]} onSuggestion={vi.fn()} onReadAttachment={vi.fn()} />);
    const greeting = await screen.findByRole("region", { name: "Loupist greets you" });
    await waitFor(() => expect(greeting).toHaveAttribute("aria-busy", "false"));
    expect(greeting.querySelectorAll(".bot-tile")).toHaveLength(0);
    quotes.mockResolvedValue([{ assetId: nvidia, price: { value: 162.85, date: "2026-11-13", currency: "USD", currencySource: "result", provider: "yfinance", sourceUrl: null, dataset: { dataset_id: "a", revision: 1, digest: "test" } } }]);
    act(() => bridge.assets.changed());
    await waitFor(() => expect(greeting.querySelectorAll(".bot-tile")).toHaveLength(1));
  });

  it("joins each follow to its own stored price by assetId", async () => {
    const { bridge, follow } = setupBridge();
    const first = `ast_${"1".repeat(16)}`, second = `ast_${"2".repeat(16)}`;
    follow([followedRow(first, "Hanul Precision"), followedRow(second, "Hanul Works")]);
    // The prices arrive in the other order.
    const price = (value: number) => ({ value, date: "2026-11-13", currency: "USD", currencySource: "result" as const, provider: "krx", sourceUrl: null, dataset: { dataset_id: "x", revision: 1, digest: "test" } });
    vi.spyOn(bridge, "watchlistQuotes").mockResolvedValue([{ assetId: second, price: price(222.25) }, { assetId: first, price: price(111.5) }]);
    render(<ChatConversation loupistId="loupist-quality" loupistName="Loupist" bridge={bridge} home messages={[]} messageKeys={[]} onSuggestion={vi.fn()} onReadAttachment={vi.fn()} />);
    const greeting = await screen.findByRole("region", { name: "Loupist greets you" });
    await waitFor(() => expect(greeting.querySelectorAll(".bot-tile")).toHaveLength(2));
    expect([...greeting.querySelectorAll(".bot-tile")].map((tile) => [tile.querySelector(".bot-tile-name")?.textContent, tile.querySelector(".bot-tile-price")?.textContent]))
      .toEqual([["Hanul Precision", expect.stringContaining("111.50")], ["Hanul Works", expect.stringContaining("222.25")]]);
  });
});

describe("Bot thread", () => {
  const evidence = { id: `ev_${"c".repeat(32)}`, title: "NVIDIA daily closes", kind: "original", url: "https://example.com/nvda", provider: "yfinance", tool_name: "equity_price_historical",
    retrieved_at: "2026-11-13T00:00:00Z", published_at: "2026-11-13", excerpt: "Close 162.85", excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [] } as NonNullable<ChatMessage["evidence_refs"]>[number];
  const step = (status: "running" | "succeeded" | "failed") => ({ id: "step-1", kind: "tool" as const, toolName: "mcp__openbb__equity_price_historical", category: "finance" as const, operation: "read" as const,
    action: "Read price history", target: "NVDA", status, href: null, startedAtMs: Date.parse("2026-11-13T00:00:01Z"), finishedAtMs: status === "running" ? null : Date.parse("2026-11-13T00:00:03Z"), resultIds: [evidence.id] });
  const question: ChatMessage = { id: "q-1", role: "user", content: "Where NVIDIA stands", status: "complete", created_at: "2026-11-13T00:00:00Z" };
  const answer = (extra: Partial<ChatMessage>): ChatMessage => ({ id: "a-1", role: "assistant", content: "", status: "complete", created_at: "2026-11-13T00:00:00Z", ...extra });
  const show = (messages: ChatMessage[], home = true) => render(<ChatConversation loupistId="loupist-quality" loupistName="Lumi" home={home} messages={messages}
    messageKeys={messages.map((message) => message.id)} onSuggestion={vi.fn()} onReadAttachment={vi.fn()} run={{ running: messages.at(-1)?.status === "streaming" }} />);
  const opened = () => {
    const events: CustomEvent[] = [];
    const listen = (event: Event) => events.push(event as CustomEvent);
    window.addEventListener("loupist:open-activity", listen);
    return { events, stop: () => window.removeEventListener("loupist:open-activity", listen) };
  };
  afterEach(() => vi.restoreAllMocks());

  it("works as one quiet line by the face that opens the activity, with no steps, chips or notes in the thread", async () => {
    const listener = opened();
    show([question, answer({ status: "streaming", progress: { startedAtMs: Date.parse("2026-11-13T00:00:00Z"), finishedAtMs: null, items: [
      { id: "note-1", kind: "commentary", text: "I will check the price first." }, step("running"),
    ] } })]);
    const line = screen.getByTestId("turn-live");
    expect(line.tagName).toBe("BUTTON");
    expect(line).toHaveTextContent("NVDA");
    // The clock ticks on screen but stays out of the name, which holds still while focused.
    const clock = line.querySelector(".bot-working-clock")!;
    expect(clock).toHaveTextContent(/\d/);
    expect(line).toHaveAccessibleName(/NVDA\. Open activity$/);
    expect(line.getAttribute("aria-label")).not.toContain(clock.textContent!);
    expect(document.querySelector('[data-testid="chat-progress"], .turn-ledger-chips')).toBeNull();
    expect(screen.queryByText("I will check the price first.")).toBeNull();
    expect(screen.getByRole("button", { name: "Open activity" }).querySelector('[data-busy="true"]')).not.toBeNull();
    await userEvent.click(line);
    expect(listener.events.at(-1)?.detail).toEqual({ messageId: "a-1" });
    listener.stop();
  });

  it("settles into the figures checked and sources, which open the sheet at the sources; a failed step never shows", async () => {
    const listener = opened();
    show([question, answer({ content: `NVIDIA closed at [$162.85](#evidence-${evidence.id}).`, evidence_refs: [evidence],
      number_checks: [{ link_index: 0, text: "$162.85", evidence_id: evidence.id, kind: "passage", status: "matched" }],
      progress: { startedAtMs: 0, finishedAtMs: 1, items: [step("succeeded"), { ...step("failed"), id: "step-2", resultIds: [] }] } })]);
    const trace = await screen.findByRole("group", { name: "Sources and checked figures for this answer" });
    expect(trace).toHaveTextContent("1 figure checked · 1 source");
    expect(document.querySelector(".chat-receipt, [data-testid='chat-progress']")).toBeNull();
    expect(screen.queryByText(/failed/i)).toBeNull();
    await userEvent.click(trace.querySelector("button")!);
    expect(listener.events.at(-1)?.detail).toEqual({ messageId: "a-1", tab: "sources" });
    listener.stop();
  });

  it("keeps a research session's step ledger and document layout", () => {
    show([question, answer({ content: "NVIDIA closed lower.", progress: { startedAtMs: 0, finishedAtMs: 1, items: [step("succeeded")] } })], false);
    expect(screen.getByTestId("chat-progress")).toBeVisible();
    expect(document.querySelector("[data-bot-message], [data-surface='bot']")).toBeNull();
  });
});

it("ends the thread with a selectable point, so a drag below the last message stops there and not in the composer", () => {
  const message = (id: string, role: ChatMessage["role"]): ChatMessage => ({ id, role, content: id, status: "complete", created_at: "2026-10-28T09:00:00Z" });
  const messages = [message("q1", "user"), message("a1", "assistant")];
  render(<>
    <ChatConversation loupistId="loupist-a" loupistName="Loupist" onSuggestion={vi.fn()} onReadAttachment={vi.fn()} messages={messages} messageKeys={messages.map((item) => item.id)} />
    <textarea data-chat-composer="" aria-label="Message" />
  </>);
  const end = document.querySelector(".thread-selection-end")!;
  expect(end).toHaveAttribute("aria-hidden", "true");
  expect(end).toBeEmptyDOMElement();
  expect(screen.getByRole("log")).toContainElement(end as HTMLElement);
  // After every message in document order, and before the composer.
  const answer = document.querySelector('[data-message-id="a1"]')!;
  expect(answer.compareDocumentPosition(end) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(end.compareDocumentPosition(screen.getByRole("textbox", { name: "Message" })) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(end.nextElementSibling).toBeNull();
});
