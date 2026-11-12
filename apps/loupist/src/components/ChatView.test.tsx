import type { ComponentProps } from "react";
import type { Chat } from "@ai-sdk/react";
import { act, render, screen, waitFor } from "@testing-library/react";
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
    isRunning: false, isStopping: false, announcement: "", error: null,
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
    onModelSelectionChange: vi.fn(), modelAuto: true, onModelResetToAuto: vi.fn(), onModelUsed: vi.fn(), onAbortRecoveredRun: vi.fn(),
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
  const watchlist = vi.spyOn(props.bridge, "watchlistList");
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
  expect(screen.getByTestId("composer-model-menu")).toHaveAccessibleName("Connect an AI account");
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
  const catalogs = [vi.spyOn(props.bridge, "agentSkillCatalog"), vi.spyOn(props.bridge, "loupistCapabilityList"), vi.spyOn(props.bridge, "marketplaceSnapshot")];
  const view = render(<TooltipProvider><ChatView {...props} /></TooltipProvider>);
  await screen.findByText("Earlier saved answer");
  await waitFor(() => { for (const read of catalogs) expect(read).toHaveBeenCalled(); });
  await act(async () => { await Promise.all(catalogs.flatMap((read) => read.mock.results.map((result) => result.value))); });
  const earlierRenders = () => rendered.mock.calls.filter(([input]) => input.text === "Earlier saved answer").length;
  const before = earlierRenders();
  await userEvent.setup().type(composer(), "follow up");
  view.rerender(<TooltipProvider><ChatView {...props} threads={[{ ...props.threads[0]!, messages: [messages[0]!, { ...messages[1]!, content: "Current answer with another token" }] }]} /></TooltipProvider>);
  expect(screen.getByText("Current answer with another token")).toBeVisible();
  expect(earlierRenders()).toBe(before);
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
