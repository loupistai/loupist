import { Chat } from "@ai-sdk/react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { TauriChatTransport, toLoupistUIMessage, type LoupistUIMessage } from "../../chat/ai-sdk";
import { useLoupistChat } from "../../chat/useLoupistChat";
import type { ChatMessage } from "../../types";
import { ChatConversation } from "./ChatConversation";
import { SafeMessageResponse } from "./SafeMessageResponse";

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
  vi.mocked(SafeMessageResponse).mockClear();

  for (const text of ["Working with sources", "Finished answer"]) {
    act(() => {
      chat.messages = chat.messages.map((item) => item.id === "answer-2"
        ? toLoupistUIMessage({ ...message("answer-2", "assistant", text), status: text === "Finished answer" ? "complete" : "streaming" }) : item);
    });
    await waitFor(() => expect(screen.getByText(text)).toBeVisible());
  }
  expect(vi.mocked(SafeMessageResponse).mock.calls.filter(([props]) => props.text === "Saved answer")).toHaveLength(0);
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

it("marks answers that finished while the window was in the background, though the chat stayed on screen (X5)", () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  try {
    window.localStorage.clear();
    vi.setSystemTime(Date.parse("2026-10-28T09:01:00Z"));
    const message = (id: string, role: ChatMessage["role"], at: string): ChatMessage => ({ id, role, content: id, status: "complete", created_at: at });
    const props = {
      loupistId: "loupist-a", chatId: "chat-blur", loupistName: "Loupist", onSuggestion: vi.fn(),
      onOpenMemory: vi.fn(), onOpenMemoryPage: vi.fn(), onOpenArtifact: vi.fn(), onOpenLink: vi.fn(), onReadAttachment: vi.fn(),
    };
    const first = [message("q1", "user", "2026-10-28T08:59:00Z"), message("a1", "assistant", "2026-10-28T09:00:00Z")];
    const view = render(<ChatConversation {...props} messages={first} messageKeys={first.map((item) => item.id)} />);
    // An answer that finishes while the user watches is not new.
    const watched = [...first, message("q2", "user", "2026-10-28T09:01:10Z"), message("a2", "assistant", "2026-10-28T09:01:30Z")];
    vi.setSystemTime(Date.parse("2026-10-28T09:01:40Z"));
    view.rerender(<ChatConversation {...props} messages={watched} messageKeys={watched.map((item) => item.id)} />);
    expect(screen.queryByRole("separator", { name: "New since you left" })).toBeNull();
    // The user switches to another app; the window stays visible.
    vi.setSystemTime(Date.parse("2026-10-28T09:02:00Z"));
    act(() => { window.dispatchEvent(new Event("blur")); });
    expect(document.visibilityState).toBe("visible");
    const later = [...watched, message("q3", "user", "2026-10-28T09:01:50Z"), message("a3", "assistant", "2026-10-28T09:05:00Z")];
    view.rerender(<ChatConversation {...props} messages={later} messageKeys={later.map((item) => item.id)} />);
    expect(screen.queryByRole("separator", { name: "New since you left" })).toBeNull();
    vi.setSystemTime(Date.parse("2026-10-28T09:06:00Z"));
    act(() => { window.dispatchEvent(new Event("focus")); });
    const divider = screen.getByRole("separator", { name: "New since you left" });
    // The divider sits right before the answer that finished in the background.
    expect(divider.nextElementSibling).toHaveAttribute("data-message-id", "a3");
  } finally {
    vi.useRealTimers();
  }
});

describe("home Thread guidance", () => {
  const post: ChatMessage = { id: "host-1", role: "assistant", content: "Your map now also tracks suppliers.", status: "complete", created_at: "2026-10-24T00:00:00Z" };
  const render_ = (messages: ChatMessage[]) => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chat = new Chat<LoupistUIMessage>({ transport: new TauriChatTransport(bridge), messages: messages.map(toLoupistUIMessage) });
    function Conversation() {
      const { messages: list, messageKeys } = useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread: undefined, setThreads: vi.fn() });
      return <ChatConversation loupistId="loupist-quality" loupistName="Loupist" bridge={bridge} home messages={list} messageKeys={messageKeys} onSuggestion={vi.fn()} onEditUser={vi.fn()}
        onOpenMemory={vi.fn()} onOpenMemoryPage={vi.fn()} onOpenArtifact={vi.fn()} onOpenLink={vi.fn()} onReadAttachment={vi.fn()} />;
    }
    return render(<Conversation />);
  };

  it("keeps the first-visit questions beside a host post until the user writes", async () => {
    render_([post]);
    expect(screen.getByText("Your map now also tracks suppliers.")).toBeVisible();
    expect(await screen.findByRole("region", { name: "You could ask" })).toBeVisible();
  });

  it("drops the first-visit questions once the user has written", async () => {
    render_([post, { ...post, id: "user-1", role: "user", content: "Hello" }]);
    expect(screen.getByText("Hello")).toBeVisible();
    expect(screen.queryByRole("region", { name: "You could ask" })).toBeNull();
  });
});
