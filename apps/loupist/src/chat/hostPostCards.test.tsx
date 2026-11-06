import { Chat } from "@ai-sdk/react";
import { render, screen } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import type { ChatMessage } from "../types";
import { TauriChatTransport, toLoupistUIMessage, type LoupistUIMessage } from "./ai-sdk";
import { useLoupistChat } from "./useLoupistChat";
import { ChatConversation } from "../components/chat/ChatConversation";

vi.mock("../components/chat/SafeMessageResponse", () => ({
  SafeMessageResponse: ({ text }: { text: string }) => <p>{text}</p>,
}));

const base = { role: "assistant" as const, status: "complete" as const, created_at: "2026-11-06T09:00:00.000Z", host_composed: true };

const posted: ChatMessage[] = [
  {
    ...base, id: "link-1", content: "New link: Alpha ↔ Gamma through Beta.",
    link: {
      connectionId: "con_1", seedName: "Alpha", farName: "Gamma",
      path: [{ name: "Beta", relation: "SUPPLIES" }, { name: "Gamma", relation: "USES" }],
      nodeIds: ["a", "b", "c"],
    },
  },
  {
    ...base, id: "vocab-1", content: "Your map now also tracks Process node.",
    vocabulary: { changeId: "change-7", producer: "preset", labels: ["Process node"], labelsKo: ["공정 노드"] },
  },
  {
    ...base, id: "handoff-1", content: "HBM4 supply widens in 2027.",
    handoff: { handoffId: "handoff-1", chatId: "chat-session", title: "Hynix HBM supply", status: "done" },
  },
  {
    ...base, id: "delivery-1", content: "The morning note.",
    delivery: {
      run_id: "run-1", task_id: "task-1", task_title: "Morning", task_kind: "briefing",
      schedule: null, run_thread_id: "run-chat", status: "succeeded", finished_at_ms: 1_700_000_000_000,
    },
  },
];

it("shows the link, vocabulary and hand-off cards a host post carries in the conversation the user sees", () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const chat = new Chat<LoupistUIMessage>({
    transport: new TauriChatTransport(bridge),
    messages: posted.map(toLoupistUIMessage),
  });
  function View() {
    const { messages, messageKeys } = useLoupistChat({
      bridge, chat, loupistId: "loupist-a", activeThread: undefined, setThreads: vi.fn(),
    });
    return <ChatConversation
      bridge={bridge} messages={messages} messageKeys={messageKeys} loupistId="loupist-a" loupistName="Chip desk"
      onSuggestion={vi.fn()} onOpenMemory={vi.fn()} onOpenMemoryPage={vi.fn()} onOpenArtifact={vi.fn()}
      onOpenLink={vi.fn()} onReadAttachment={vi.fn()} onOpenRunChat={vi.fn()}
    />;
  }
  render(<View />);
  const seen = {
    lookInto: screen.queryByRole("button", { name: "Look into it" }) !== null,
    notUseful: screen.queryByRole("button", { name: "Not useful" }) !== null,
    showPath: screen.queryByRole("button", { name: "Show this path on the map" }) !== null,
    undo: screen.queryByRole("button", { name: "Undo" }) !== null,
    open: screen.queryAllByRole("button", { name: "Open" }).length,
    researching: screen.queryByText("Researched: Hynix HBM supply") !== null,
  };
  expect(seen).toEqual({ lookInto: true, notUseful: true, showPath: true, undo: true, open: 2, researching: true });
});
