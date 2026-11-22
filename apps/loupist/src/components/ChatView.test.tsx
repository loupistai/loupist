import type { ComponentProps } from "react";
import type { Chat } from "@ai-sdk/react";
import i18n from "@/i18n";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../bridge/mockBridge";
import type { LoupistUIMessage } from "../chat/ai-sdk";
import { MOCK_LOUPISTS } from "../bridge/mock/fixtures";
import type { ChatMessage, ChatThread } from "../types";
import { TooltipProvider } from "./ui/tooltip";
import { ChatView } from "./ChatView";
import * as messageRenderer from "./chat/SafeMessageResponse";
import { renderWithOpen, destinations } from "../test/open";
import { evidenceFixture } from "../test/evidenceFixtures";

const actions = vi.hoisted(() => ({
  saveFeedback: vi.fn(), submit: vi.fn(), steer: vi.fn(), commitSteers: vi.fn(), abort: vi.fn(),
}));

vi.mock("../chat/useLoupistChat", () => ({
  useLoupistChat: ({ activeThread }: { activeThread?: ChatThread }) => ({
    messages: activeThread?.messages ?? [],
    messageKeys: activeThread?.messages.map((message) => message.id) ?? [],
    isRunning: activeThread?.messages.at(-1)?.status === "streaming", isStopping: false, announcement: "", error: null,
    ...actions,
  }),
}));

beforeEach(() => { actions.submit.mockReset(); actions.saveFeedback.mockReset(); });

function viewProps(messages: ChatMessage[], extra: Partial<ComponentProps<typeof ChatView>> = {}): ComponentProps<typeof ChatView> {
  const thread: ChatThread = { id: "chat", loupist_id: "loupist-quality", title: "Research", updated_at: "2026-10-03T00:00:00Z", use_memory: false, update_memory: false, messages };
  return {
    bridge: new MockLoupistBridge({ delay_ms: 0 }), chat: { id: thread.id } as Chat<LoupistUIMessage>, loupist: MOCK_LOUPISTS[0]!,
    threads: [thread], setThreads: vi.fn(), activeThreadId: thread.id, setActiveThreadId: vi.fn(),
    onCreateThread: vi.fn(), knowledgePins: [], onRemoveKnowledgePin: vi.fn(),
    modelCatalog: null, modelSelection: { model_profile_id: "", thinking_level: "off", run_options: {} },
    onModelSelectionChange: vi.fn(), modelIsDefault: true, onChooseDefault: vi.fn(), onModelUsed: vi.fn(), onAbortRecoveredRun: vi.fn(),
    ...extra,
  };
}
const composer = () => screen.getByRole("textbox", { name: "Message Loupist" }) as HTMLTextAreaElement;

it("releases loaded images when leaving a conversation", async () => {
  const props = viewProps([{ id: "image-question", role: "user", status: "complete", content: "Review this", created_at: "2026-10-03T00:00:00Z", attachments: [
    { id: "image", filename: "chart.png", media_type: "image/png", size_bytes: 12 },
  ] }]);
  const read = vi.spyOn(props.bridge, "chatAttachmentRead").mockResolvedValue({ data_url: "data:image/png;base64,aTE=" });
  const other = { ...props.threads[0]!, id: "other", messages: [] };
  props.threads.push(other);
  const view = render(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
  await waitFor(() => expect(read).toHaveBeenCalledOnce());
  view.rerender(<TooltipProvider><ChatView {...props} activeThreadId={other.id} chat={{ id: other.id } as Chat<LoupistUIMessage>} /></TooltipProvider>);
  view.rerender(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
});

it("keeps historical ticker messages and new dollar text ordinary without fetching assets or the watchlist", async () => {
  const text = "$AAPL:NASDAQ (Apple) What changed? It cost $5.";
  const props = viewProps([
    { id: "ticker-question", role: "user", status: "complete", content: text, created_at: "2026-10-03T00:00:00Z" },
    { id: "ticker-answer", role: "assistant", status: "complete", content: "Operating margin increased.", created_at: "2026-10-03T00:00:01Z" },
  ]);
  const watchlist = vi.spyOn(props.bridge, "assetsList");
  const asset = vi.spyOn(props.bridge, "assetPageGet");
  render(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
  expect(await screen.findByText(text)).toBeVisible();
  expect(await screen.findByText("Operating margin increased.")).toBeVisible();
  expect(screen.queryByRole("region", { name: "About Apple" })).not.toBeInTheDocument();
  expect(screen.queryByTitle("AAPL:NASDAQ")).not.toBeInTheDocument();
  await userEvent.setup().type(composer(), "Compare $NVDA and $5");
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 150)); });
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  expect(watchlist).not.toHaveBeenCalled();
  expect(asset).not.toHaveBeenCalled();
});

it("opens a linked page the Loupist already kept as its retained copy, and other links as pages", async () => {
  const kept = evidenceFixture({ id: "ev_kept", url: "https://hanul.example/q3" });
  const props = viewProps([
    { id: "question", role: "user", status: "complete", content: "What changed?", created_at: "2026-10-03T00:00:00Z" },
    { id: "answer", role: "assistant", status: "complete", created_at: "2026-10-03T00:00:01Z", evidence_refs: [kept],
      content: "See [the quarter](https://hanul.example/q3#margin) and [the peer](https://other.example/)." },
  ]);
  const { open } = renderWithOpen(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
  await userEvent.click(await screen.findByRole("link", { name: "the quarter" }));
  expect(destinations(open).at(-1)).toEqual({ kind: "evidence", evidence: kept });
  await userEvent.click(screen.getByRole("link", { name: "the peer" }));
  expect(destinations(open).at(-1)).toEqual({ kind: "link", url: "https://other.example/" });
});

it("keeps a saved Chat readable without a model and opens settings instead of sending", async () => {
  const user = userEvent.setup();
  const onOpenModelSettings = vi.fn();
  const onDraftChange = vi.fn();
  const props = viewProps([{ id: "saved-answer", role: "assistant", status: "complete", content: "This saved answer remains available.", created_at: "2026-10-03T00:00:00Z" }],
    { modelSetupMessage: "Connect or show a model to send messages.", onOpenModelSettings, initialPrompt: "Draft", onDraftChange });
  const create = vi.spyOn(props.bridge, "chatCreate");
  const view = render(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
  // The renderer may replace its plain fallback after it loads; check the node that stays.
  await waitFor(() => expect(screen.getByText("This saved answer remains available.")).toBeVisible());
  expect(composer()).toHaveValue("Draft");
  await user.type(composer(), " more");
  expect(onDraftChange).toHaveBeenLastCalledWith("Draft more");
  expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
  expect(screen.getByTestId("composer-model-menu")).toHaveAccessibleName("No AI account");
  await user.click(screen.getByRole("button", { name: "Open Settings" }));
  expect(onOpenModelSettings).toHaveBeenCalledOnce();
  expect(actions.submit).not.toHaveBeenCalled();
  expect(create).not.toHaveBeenCalled();
  view.rerender(<TooltipProvider><ChatView {...props} modelSetupMessage={undefined} /></TooltipProvider>);
  expect(screen.queryByRole("button", { name: "Open Settings" })).toBeNull();
});

it("does not rerender an unchanged earlier answer while typing or streaming another answer", async () => {
  const rendered = vi.spyOn(messageRenderer, "SafeMessageResponse").mockImplementation(({ text }) => <p>{text}</p>);
  const messages: ChatMessage[] = [
    { id: "earlier-answer", role: "assistant", status: "complete", content: "Earlier saved answer", created_at: "2026-10-03T00:00:00Z" },
    { id: "streaming-answer", role: "assistant", status: "streaming", content: "Current answer", created_at: "2026-10-03T00:00:01Z" },
  ];
  const props = viewProps(messages);
  const catalogs = [vi.spyOn(props.bridge, "skillsList"), vi.spyOn(props.bridge, "loupistCapabilityList"), vi.spyOn(props.bridge, "marketplaceSnapshot")];
  try {
    const view = render(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
    await screen.findByText("Earlier saved answer");
    await waitFor(() => { for (const read of catalogs) expect(read).toHaveBeenCalled(); });
    await act(async () => { await Promise.all(catalogs.flatMap((read) => read.mock.results.map((result) => result.value))); });
    const earlierRenders = () => rendered.mock.calls.filter(([input]) => input.text === "Earlier saved answer").length;
    const before = earlierRenders();
    await userEvent.setup().type(composer(), "follow up");
    view.rerender(<TooltipProvider><ChatView {...props} threads={[{ ...props.threads[0]!, messages: [messages[0]!, { ...messages[1]!, content: "Current answer with another token" }] }]} /></TooltipProvider>);
    // The reveal governor shows the new words a moment later, at an even pace.
    expect(await screen.findByText("Current answer with another token")).toBeVisible();
    expect(earlierRenders()).toBe(before);
  } finally {
    rendered.mockRestore();
  }
});

describe("teaching from feedback", () => {
  const corrected = (): ChatMessage => ({ id: "answer-feedback", role: "assistant", status: "complete", content: "Profit rose 12%.", created_at: "2026-10-18T00:00:00Z", feedback: { rating: "down", correction: "Use consolidated profit.", updatedAt: 1 } });
  const feedbackProps = async (extra: Partial<ComponentProps<typeof ChatView>> = {}) => {
    const props = viewProps([corrected()], { initialPrompt: "Existing draft", ...extra });
    return { ...props, modelCatalog: await props.bridge.modelCatalogGet(), modelSelection: { model_profile_id: "model-test", thinking_level: "low", run_options: {} } };
  };

  it("prefills an editable teaching turn once, preserving the draft, and never submits it", async () => {
    const user = userEvent.setup();
    render(<TooltipProvider><ChatView {...await feedbackProps()} /></TooltipProvider>);
    await user.click(await screen.findByRole("button", { name: "Teach the Loupist" }));
    expect(composer().value).toContain("Existing draft");
    expect(composer().value).toContain("> Profit rose 12%.");
    expect(composer().value).toContain("> Use consolidated profit.");
    await user.type(composer(), " Additional guidance");
    await user.click(screen.getByRole("button", { name: "Teach the Loupist" }));
    expect(composer().value.split("> Use consolidated profit.").length - 1).toBe(1);
    expect(composer().value).toContain("Additional guidance");
    expect(actions.submit).not.toHaveBeenCalled();
    expect(actions.saveFeedback).not.toHaveBeenCalled();
  });

  it("can teach again after sending, and in another Chat with its own draft", async () => {
    const user = userEvent.setup();
    actions.submit.mockResolvedValue({ accepted: true });
    const props = await feedbackProps();
    const other: ChatThread = { ...props.threads[0]!, id: "other-feedback-chat" };
    props.threads = [...props.threads, other];
    const view = render(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
    const teach = await screen.findByRole("button", { name: "Teach the Loupist" });
    await user.click(teach);
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(actions.submit).toHaveBeenCalledOnce());
    expect(composer()).toHaveValue("");
    await user.click(teach);
    expect(composer().value).toContain("> Use consolidated profit.");
    view.rerender(<TooltipProvider><ChatView {...props} activeThreadId={other.id} chat={{ id: other.id } as Chat<LoupistUIMessage>} initialPrompt="Other draft" /></TooltipProvider>);
    await user.click(screen.getByRole("button", { name: "Teach the Loupist" }));
    expect(composer().value.startsWith("Other draft\n\n")).toBe(true);
    expect(actions.submit).toHaveBeenCalledOnce();
  });
});

describe("a link's Look into it in Bot", () => {
  const link = (connectionId: string): ChatMessage => ({
    id: `link-${connectionId}`, role: "assistant", status: "complete", content: "New link.", created_at: "2026-11-18T09:00:00Z", host_composed: true,
    link: { connectionId, seedName: `Seed ${connectionId}`, farName: "Far", path: [{ name: "Far", relation: "SUPPLIES" }], nodeIds: ["a", "b"] },
  });
  const botProps = async (extra: Partial<ComponentProps<typeof ChatView>> = {}) => {
    const props = viewProps([link("con_1"), link("con_2")], { initialPrompt: "Unsent draft", ...extra });
    props.threads = [{ ...props.threads[0]!, home: true }];
    return { ...props, modelCatalog: await props.bridge.modelCatalogGet(), modelSelection: { model_profile_id: "model-test", thinking_level: "low", run_options: {} }, modelIsDefault: false, ...extra };
  };
  const lookInto = (seed: string) => within(screen.getByRole("article", { name: `New link: ${seed} and Far` })).getByRole("button", { name: "Look into it" });

  it("runs with the composer's model, level and Memory choice, not the draft, and follows a changed choice", async () => {
    const user = userEvent.setup();
    const props = await botProps();
    const start = vi.spyOn(props.bridge, "researchHandoffStart").mockResolvedValue({ chatId: "chat-9", handoffId: "handoff-9" });
    const view = render(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
    await user.type(composer(), " changed");
    await user.click(await screen.findAllByRole("button", { name: "Look into it" }).then(() => lookInto("Seed con_1")));
    // The Bot thread saves to Memory off (use_memory/update_memory false): false is sent as false.
    expect(start).toHaveBeenLastCalledWith({ loupistId: "loupist-quality", connectionId: "con_1",
      modelProfileId: "model-test", thinkingLevel: "low", runOptions: {}, useMemory: false, updateMemory: false });
    expect(composer()).toHaveValue("Unsent draft changed");
    expect(actions.submit).not.toHaveBeenCalled();

    view.rerender(<TooltipProvider><ChatView {...props} modelSelection={{ model_profile_id: "model-test", thinking_level: "high", run_options: { verbosity: "low" } }} /></TooltipProvider>);
    await user.click(lookInto("Seed con_2"));
    expect(start).toHaveBeenLastCalledWith(expect.objectContaining({ connectionId: "con_2", thinkingLevel: "high", runOptions: { verbosity: "low" } }));
    expect(start).toHaveBeenCalledTimes(2);
  });

  it("reads a link whose session is at work as opened, though Bot posts nothing for it yet", async () => {
    const props = await botProps();
    props.threads = [...props.threads, { ...props.threads[0]!, id: "chat-link", home: false, title: "Link: Seed con_1 and Far", messages: [],
      handoff_run: { handoff_id: "handoff-7", run_id: "handoff-7a8b9c0d", connection_id: "con_1" } }];
    render(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
    const working = screen.getByRole("article", { name: "New link: Seed con_1 and Far" });
    expect(within(working).getByText("Researching it. The result will arrive here.")).toBeVisible();
    expect(within(working).queryByRole("button", { name: "Look into it" })).toBeNull();
    expect(within(working).queryByRole("button", { name: "Not useful" })).toBeNull();
    expect(lookInto("Seed con_2")).toBeVisible();
    expect(screen.queryByRole("article", { name: /^Research: / })).toBeNull();
  });

  it("keeps the model unset on a default send", async () => {
    const props = await botProps({ hostAuto: true, modelIsDefault: true });
    const start = vi.spyOn(props.bridge, "researchHandoffStart").mockResolvedValue({ chatId: "chat-9", handoffId: "handoff-9" });
    render(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
    await userEvent.setup().click(await screen.findAllByRole("button", { name: "Look into it" }).then(() => lookInto("Seed con_1")));
    expect(start).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-quality", connectionId: "con_1",
      modelProfileId: "", thinkingLevel: "", runOptions: {}, useMemory: false, updateMemory: false });
  });
});

describe("a research session Bot handed off", () => {
  const RUN = "handoff-1a2b3c4d";
  // While it runs, the host reports the run on the session and Bot posts nothing; once it ends, Bot posts the result.
  const result: ChatMessage = {
    id: "handoff-card", role: "assistant", status: "complete", content: "HBM4 supply widens in 2027.", created_at: "2026-11-18T09:10:00Z", host_composed: true,
    handoff: { handoffId: "handoff-1", chatId: "chat", title: "Hynix HBM supply", status: "done" },
  };
  const sessionProps = (status: "running" | "done", watch = vi.fn<NonNullable<ComponentProps<typeof ChatView>["onWatchHostedRun"]>>()) => {
    const props = viewProps([{ id: "request", role: "user", status: "complete", content: "How tight is HBM4 supply into 2027?", created_at: "2026-11-18T09:00:00Z" }], { onWatchHostedRun: watch });
    const bot: ChatThread = { ...props.threads[0]!, id: "home-loupist-quality", home: true, messages: status === "done" ? [result] : [] };
    const session: ChatThread = { ...props.threads[0]!, ...(status === "running" ? { handoff_run: { handoff_id: "handoff-1", run_id: RUN } } : {}) };
    return { ...props, threads: [bot, session] };
  };
  const botView = (props: ComponentProps<typeof ChatView>) => <TooltipProvider><ChatView {...props} activeThreadId="home-loupist-quality" chat={{ id: "home-loupist-quality" } as Chat<LoupistUIMessage>} /></TooltipProvider>;

  it("links only the originating Bot reply to its session through running, completion and removal", async () => {
    const props = sessionProps("running", vi.fn());
    const [bot, session] = props.threads;
    const reply: ChatMessage = { id: "origin", role: "assistant", content: "Handed off.", created_at: result.created_at, research_session_id: session!.id };
    const home = { ...bot!, messages: [{ ...reply, id: "unrelated", content: "Earlier reply.", research_session_id: undefined }, reply] };
    const show = (sessions: ChatThread[]) => botView({ ...props, threads: [home, ...sessions] });
    const view = renderWithOpen(show([session!]));
    const buttonInReply = () => within(screen.getByText("Handed off.").closest("article")!);
    const card = buttonInReply().getByRole("button", { name: `${session!.title} Researching it. The result will arrive here.` });
    expect(within(screen.getByText("Earlier reply.").closest("article")!).queryByText(session!.title)).toBeNull();
    const user = userEvent.setup();
    card.focus();
    await user.keyboard("{Enter}");
    await user.keyboard(" ");
    await user.click(card);
    expect(destinations(view.open)).toEqual(Array(3).fill({ kind: "chat", threadId: session!.id }));

    view.rerender(show([{ ...session!, title: "Revised title", handoff_run: undefined }]));
    expect(buttonInReply().getByRole("button", { name: "Revised title" })).toBeVisible();
    expect(buttonInReply().queryByText(/Researching it/)).toBeNull();
    view.rerender(show([]));
    expect(buttonInReply().queryByRole("button", { name: "Revised title" })).toBeNull();
  });

  it("joins the run while the session is shown and leaves it, still running, when Bot is shown again", async () => {
    const release = vi.fn();
    const watch = vi.fn<NonNullable<ComponentProps<typeof ChatView>["onWatchHostedRun"]>>((_threadId, onPhase) => { onPhase("live"); return release; });
    const view = render(<TooltipProvider><ChatView {...sessionProps("running", watch)} /></TooltipProvider>);
    await waitFor(() => expect(watch).toHaveBeenCalledOnce());
    expect(watch).toHaveBeenCalledWith("chat", expect.any(Function));
    // The host stops reporting the run before the stream's own last event: the join stays to finish it.
    view.rerender(<TooltipProvider><ChatView {...sessionProps("done", watch)} /></TooltipProvider>);
    expect(release).not.toHaveBeenCalled();
    view.rerender(botView(sessionProps("done", watch)));
    expect(release).toHaveBeenCalledOnce();
    expect(watch).toHaveBeenCalledOnce();
  });

  it("guides the run from the composer while joining, with no model chosen, and stops it by its id", async () => {
    const user = userEvent.setup();
    actions.steer.mockResolvedValue({ id: "guide", text: "Focus on Samsung too.", createdAt: "2026-11-18T09:02:00Z" });
    const props = sessionProps("running", vi.fn(() => () => {}));
    const abort = vi.spyOn(props.bridge, "chatAbort").mockResolvedValue();
    render(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
    await user.type(composer(), "Focus on Samsung too.");
    await user.click(screen.getByRole("button", { name: "Guide this response" }));
    await waitFor(() => expect(actions.steer).toHaveBeenCalledExactlyOnceWith("Focus on Samsung too."));
    expect(actions.commitSteers).toHaveBeenCalledWith([{ id: "guide", text: "Focus on Samsung too.", createdAt: "2026-11-18T09:02:00Z" }]);
    expect(actions.submit).not.toHaveBeenCalled();
    await waitFor(() => expect(composer()).toHaveValue(""));
    await user.click(screen.getByRole("button", { name: "Stop response" }));
    expect(abort).toHaveBeenCalledExactlyOnceWith(RUN);
    expect(props.onAbortRecoveredRun).not.toHaveBeenCalled();
  });

  it("shows the stored session without joining when the run finished, and once the join found nothing running", async () => {
    const watch = vi.fn<NonNullable<ComponentProps<typeof ChatView>["onWatchHostedRun"]>>();
    const view = render(<TooltipProvider><ChatView {...sessionProps("done", watch)} /></TooltipProvider>);
    expect(await screen.findByText("How tight is HBM4 supply into 2027?")).toBeVisible();
    expect(watch).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Stop response" })).toBeNull();
    watch.mockImplementation((_threadId, onPhase) => { onPhase("attaching"); onPhase("idle"); return () => {}; });
    view.rerender(<TooltipProvider><ChatView {...sessionProps("running", watch)} /></TooltipProvider>);
    await waitFor(() => expect(watch).toHaveBeenCalledOnce());
    expect(screen.queryByRole("button", { name: "Stop response" })).toBeNull();
  });

  it("never joins Bot itself", () => {
    const watch = vi.fn<NonNullable<ComponentProps<typeof ChatView>["onWatchHostedRun"]>>();
    render(botView(sessionProps("running", watch)));
    expect(watch).not.toHaveBeenCalled();
  });
});

it("keeps Bot task cards in sync with Schedule through pauses and deletion", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = MOCK_LOUPISTS[0]!.id;
  const task = await bridge.proposeScheduledTask({
    loupist_id: loupistId, title: "Compare inventory days", prompt: "Compare inventory days",
    schedule: { kind: "daily", hour: 8, minute: 0, timezone: "Asia/Seoul" }, target: { kind: "new_chat" },
    model_profile_id: "model", thinking_level: "medium", run_options: {}, use_memory: true, update_memory: false, status: "active",
  });
  const identity = { loupist_id: loupistId, task_id: task.id };
  const active = await bridge.scheduledTaskConfirm(identity);
  const props = viewProps([{ id: "answer", role: "assistant", status: "complete", created_at: "2026-10-03T00:00:00Z", content: "Scheduled.",
    schedule_receipts: [{ ...active, task_id: task.id, action: "created" }] }], { bridge });
  Object.assign(props.threads[0]!, { home: true, update_memory: true, use_memory: true });
  renderWithOpen(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
  const card = within(screen.getByRole("article", { name: "Scheduled task: Compare inventory days" }));
  expect(card.queryByText("Active")).toBeNull();
  expect(await card.findByText("Active")).toBeVisible();
  expect(card.getByText(/^Next run /)).toBeVisible();
  const paused = await act(() => bridge.scheduledTaskSetStatus({ ...identity, expected_revision: active.revision, status: "paused" }));
  expect(await card.findByText("Paused")).toBeVisible();
  expect(card.queryByText(/^Next run /)).toBeNull();
  await act(() => bridge.scheduledTaskDelete({ ...identity, expected_revision: paused.revision }));
  expect(await card.findByText("Deleted")).toBeVisible();
  expect(card.getByText(task.title)).toBeVisible();
  expect(card.queryByRole("button")).toBeNull();
});

it("uses Schedule's localized title on Bot cards and keeps a custom task with the same title", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const task = await bridge.proposeScheduledTask({
    loupist_id: MOCK_LOUPISTS[0]!.id, title: "Briefings", prompt: "Report", schedule: { kind: "interval", every_minutes: 60 },
    target: { kind: "new_chat" }, model_profile_id: "model", thinking_level: "medium", run_options: {}, use_memory: true, update_memory: false, status: "active",
  });
  bridge.installScheduledTask({ ...task, kind: "briefing", id: "built-in" });
  const props = viewProps([{ id: "answer", role: "assistant", status: "complete", created_at: "2026-10-03T00:00:00Z", content: "Scheduled.",
    schedule_receipts: [task.id, "built-in"].map(id => ({ ...task, task_id: id, action: "viewed" })) }], { bridge });
  Object.assign(props.threads[0]!, { home: true, update_memory: true, use_memory: true });
  await i18n.changeLanguage("ko");
  const view = renderWithOpen(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
  try {
    expect(await screen.findByText("브리핑", { selector: ".message-schedule-heading strong" })).toBeVisible();
    expect(screen.getByText("Briefings", { selector: ".message-schedule-heading strong" })).toBeVisible();
  } finally {
    view.unmount();
    await i18n.changeLanguage("en");
  }
});

it("explains the feedback lock for each reply and keeps finished answers readable during a run", async () => {
  const created_at = "2026-10-03T00:00:00Z";
  const source = evidenceFixture({ id: "ev_filing", title: "Annual filing", excerpt: "The closing price was $42.10." });
  const props = viewProps([
    { id: "question", created_at, role: "user", content: "What is a closing price?", status: "complete" },
    { id: "finished", created_at, role: "assistant", content: "The final traded price was [$42.30](#evidence-ev_filing).", status: "complete",
      memory_update: { status: "applied", commitId: "save", changes: [{ kind: "Source", recordId: "filing", title: "Saved filing", operation: "create",
        lesson: "", basis: "", futureUse: "", knowledge: { kind: "source", recordId: "filing", revisionId: "rev", objectId: "filing" } }] },
      evidence_refs: [source], number_checks: [{ text: "$42.30", evidence_id: source.id, link_index: 0, status: "not_found", kind: "passage" }] },
    { id: "followup", created_at, role: "user", content: "And last year?", status: "complete" },
    { id: "running", created_at, role: "assistant", content: "Looking at last year.", status: "streaming" },
  ]);
  Object.assign(props.threads[0]!, { home: true, update_memory: true, use_memory: true });
  const user = userEvent.setup();
  const { open } = renderWithOpen(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
  const earlier = within((await screen.findByTestId("number-chip")).closest("article")!);
  const live = within((await screen.findByText("Looking at last year.")).closest("article")!);
  for (const name of ["Helpful", "Not helpful", "Suggest a correction"]) {
    expect(earlier.getByRole("button", { name })).toBeDisabled();
    expect(earlier.getByRole("button", { name })).toHaveAccessibleDescription("Available when the current reply finishes.");
    expect(live.getByRole("button", { name })).toBeDisabled();
    expect(live.getByRole("button", { name })).toHaveAccessibleDescription("Available when the reply finishes");
  }
  expect(actions.saveFeedback).not.toHaveBeenCalled();
  await user.click(earlier.getByRole("button", { name: "Copy answer" }));
  expect(await navigator.clipboard.readText()).toContain("$42.30");
  const edit = screen.getAllByRole("button", { name: "Edit message" })[0]!;
  expect(edit).toBeDisabled();
  expect(edit).toHaveAttribute("title", "Available when the current reply finishes.");
  await user.click(earlier.getByTestId("number-chip"));
  expect(await screen.findByRole("dialog")).toHaveTextContent("Annual filing");
  await user.keyboard("{Escape}");
  await user.click(earlier.getByRole("button", { name: "Saved 1 source" }));
  expect(earlier.getByRole("list", { name: "Saved in this answer" })).toHaveTextContent("Saved filing");
  await user.click(earlier.getByRole("button", { name: "1 source" }));
  const activity = await screen.findByTestId("activity-sheet");
  const sources = within(activity).getByRole("heading", { name: /What it read/ }).closest("section")!;
  await user.click(within(sources).getByRole("button", { name: /Annual filing/ }));
  expect(destinations(open).at(-1)).toMatchObject({ kind: "evidence", evidence: { id: source.id } });
});
