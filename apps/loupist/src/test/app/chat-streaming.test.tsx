import { onTestFinished } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ChatStreamEvent } from "../../types";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { chooseLoupist, loupistIsRunning, newChatButton, openApp, privateChatToggle, waitForLoupistRunning, openChat } from "../renderApp";

type User = ReturnType<typeof userEvent.setup>;
const send = async (user: User, text: string) => {
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), text);
  await user.click(screen.getByRole("button", { name: "Send" }));
};
const terminal = { created_at: "2026-08-25T00:00:01.000Z",
  execution_model: { profile_id: "model-test", name: "GPT-5.6 Luna", provider: "openai-codex", model: "gpt-5.6-luna", thinking_level: "medium", run_options: {} },
  agent_harness: { schema: "loupist-agent-harness/1", mode: "chat" as const, skill_ids: [], capability_ids: [], digest: "b".repeat(64) },
};

describe("Loupist · Chat streaming and concurrency", () => {
  it("shows the submitted prompt and live status before progress arrives", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
      observer({ type: "started", run_id: request.run_id });
      return { run_id: request.run_id };
    });
    await openApp(bridge);
    await send(user, "오늘 하이닉스 어땠어?");
    expect(await screen.findByText("오늘 하이닉스 어땠어?")).toBeVisible();
    expect(await screen.findByLabelText("Response status")).toHaveTextContent("Waiting for the model");
    expect(screen.getByRole("button", { name: "Stop response" })).toBeVisible();
  });

  it("shows short steps after the preamble and folds them when the final answer arrives", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    let finishRun!: () => void;
    const gate = new Promise<void>((resolve) => { finishRun = resolve; });
    vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
      observer({ type: "started", run_id: request.run_id });
      observer({ type: "progress", run_id: request.run_id, progress: { startedAtMs: 1, items: [
        { id: "commentary-1", kind: "commentary", text: "Tool preamble that must not become the final answer." },
        { id: "system-2", kind: "system", category: "system", operation: "compact", action: "Summarized earlier messages", status: "succeeded" },
      ] } });
      await gate;
      observer({ type: "completed", run_id: request.run_id, message_id: "message-final", final_text: "Authoritative final answer.", ...terminal });
      return { run_id: request.run_id };
    });
    await send(user, "Return only the settled final answer");
    const progress = await screen.findByLabelText("Work progress");
    expect(await screen.findByText("Tool preamble that must not become the final answer.")).toBeVisible();
    // The steps stay inside the row, before and after the answer; the row keeps its height.
    expect(within(progress).getByRole("button")).toHaveAttribute("aria-expanded", "false");
    expect(progress).not.toHaveTextContent("Reviewed earlier conversation");
    expect(screen.queryByText("Authoritative final answer.")).not.toBeInTheDocument();
    finishRun();
    const answer = await screen.findByText("Authoritative final answer.");
    const message = answer.closest("article")!;
    await waitFor(() => expect(within(progress).getByRole("button")).toHaveAttribute("aria-expanded", "false"));
    await user.click(within(progress).getByRole("button"));
    expect(within(progress).getByText("Reviewed earlier conversation")).toBeVisible();
    expect(progress).not.toHaveTextContent("Summarized earlier messages");
    expect(within(message).getByText("Tool preamble that must not become the final answer.")).toBeVisible();
  });

  it("streams Markdown in the answer body across tool use, guidance and settlement", async () => {
    const matchMedia = window.matchMedia;
    vi.stubGlobal("matchMedia", (query: string) => ({
      ...matchMedia(query), matches: query === "(prefers-reduced-motion: no-preference)",
    }));
    onTestFinished(() => { vi.unstubAllGlobals(); });
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const chatSend = vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
      observer({ type: "started", run_id: request.run_id });
      return { run_id: request.run_id };
    });
    await openApp(bridge);
    await send(user, "분기 보고서를 확인해 줘");
    const [{ run_id }, observer] = chatSend.mock.calls[0]!;
    const emit = (event: ChatStreamEvent) => act(async () => observer(event));
    await emit({ type: "phase", run_id, phase: "thinking" });
    await waitFor(() => expect(screen.getByLabelText("Response status")).toHaveTextContent("Thinking it through"));
    await emit({ type: "phase", run_id, phase: "writing" });
    const preamble = { id: "note", kind: "commentary" as const, text: "보고서를 확인할게요." };
    await emit({ type: "progress", run_id, sequence: 1, progress: { startedAtMs: Date.now(), items: [preamble] } });
    await waitFor(() => expect(document.querySelector(".message.streaming .message-content")).toHaveTextContent(preamble.text));
    const originalNote = screen.getByText(preamble.text);
    await emit({ type: "phase", run_id, phase: "tool" });
    const read = { id: "read", kind: "tool" as const, category: "files" as const, operation: "read" as const, action: "Read filing", status: "succeeded" as const };
    await emit({ type: "progress_patch", run_id, patch: { sequence: 2, upsertItems: [read] } });
    const history = await screen.findByLabelText("Work progress");
    expect(screen.getByText(preamble.text)).toBe(originalNote);
    expect(history).not.toContainElement(originalNote);
    expect(within(history).getByRole("button")).toHaveAttribute("aria-expanded", "false");
    await emit({ type: "phase", run_id, phase: "writing" });
    const heading = "## 영업이익";
    const write = (sequence: number, text: string, id = "draft") => emit({
      type: "progress_patch", run_id, patch: { sequence, upsertItems: [{ id, kind: "commentary", text }] },
    });
    await write(3, heading);
    const title = await screen.findByRole("heading", { name: "영업이익" });
    const body = title.closest<HTMLElement>(".message-content")!;
    expect(body).not.toBeNull();
    expect(history).not.toContainElement(title);
    const toggle = within(history).getByRole("button");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(within(history).getByText("Read filing")).toBeVisible();
    expect(originalNote).toBeVisible();
    const first = `${heading}\n\n전년보다 **늘었습니다**.`;
    await write(4, first);
    const newWord = await within(body).findByText("늘었습니다");
    expect(newWord.closest('[data-streamdown="strong"]')).not.toBeNull();
    expect(originalNote.compareDocumentPosition(history) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(history.compareDocumentPosition(body) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // The settled delta can include text from the last, unflushed progress window; it names the draft it replaces.
    const settled = `${first}\n\n보고서 기준입니다.`;
    await emit({ type: "delta", run_id, text: settled, settles: "draft" });
    await waitFor(() => expect(body).toHaveTextContent("보고서 기준입니다."));
    expect(screen.getAllByRole("heading", { name: "영업이익" })).toEqual([title]);
    await emit({ type: "progress_patch", run_id, patch: { sequence: 5, removeItemIds: ["draft"] } });
    const more = "추가 질문도 확인했어요.";
    await write(6, more, "continued");
    await waitFor(() => expect(body).toHaveTextContent(more));
    await emit({ type: "delta", run_id, text: `\n\n${more}`, settles: "continued" });
    await emit({ type: "progress_patch", run_id, patch: { sequence: 7, removeItemIds: ["continued"] } });
    await emit({ type: "completed", run_id, message_id: "final", final_text: `${settled}\n\n${more}`, ...terminal });
    expect(await within(body.closest("article")!).findByRole("button", { name: "Copy answer" })).toBeVisible();
    expect(within(body).getByRole("heading", { name: "영업이익" })).toBe(title);
    expect(body.textContent!.split(more)).toHaveLength(2);
    expect(originalNote).toBeVisible();
  });

  it("settles repeated partial deltas at a canonical terminal error", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
      observer({ type: "started", run_id: request.run_id });
      for (let line = 1; line <= 10; line += 1) observer({ type: "delta", run_id: request.run_id, text: `Partial line ${line}.\n` });
      observer({ type: "error", run_id: request.run_id, message: "Response could not be completed.", message_id: "assistant-error", final_text: "Response could not be completed.", ...terminal });
      return { run_id: request.run_id };
    });
    await openApp(bridge);
    await send(user, "Return a terminal error after streaming.");
    const failure = await screen.findByRole("alert");
    expect(failure.closest("article")).toHaveClass("error");
    expect(screen.queryByText("Response could not be completed.")).not.toBeInTheDocument();
    expect(screen.queryByText("Partial line 10.")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message Loupist" })).toBeEnabled());
  });

  it("runs distinct Loupists concurrently and stops only the selected run", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const release = bridge.pauseChatStreams();
    try {
      const chatSend = vi.spyOn(bridge, "chatSend");
      const chatAbort = vi.spyOn(bridge, "chatAbort");
      await openApp(bridge);
      await send(user, "Keep the quality run active");
      await waitForLoupistRunning("Quality Compounder");
      await chooseLoupist(user, "Contrarian Value");
      await openChat("Downside scenario review");
      await send(user, "Keep the value run active too");
      await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2));
      expect(loupistIsRunning("Quality Compounder")).toBe(true);
      expect(loupistIsRunning("Contrarian Value")).toBe(true);

      const [first, second] = await Promise.all(chatSend.mock.results.map((result) => result.value));
      await chooseLoupist(user, "Quality Compounder");
      await openChat();
      await user.click(await screen.findByRole("button", { name: "Stop response" }));
      await waitFor(() => expect(chatAbort).toHaveBeenCalledWith(first.run_id));
      expect(chatAbort).not.toHaveBeenCalledWith(second.run_id);
      await chooseLoupist(user, "Contrarian Value");
      await openChat("Downside scenario review");
      await user.click(await screen.findByRole("button", { name: "Stop response" }));
      await waitFor(() => expect(chatAbort).toHaveBeenCalledWith(second.run_id));
      await waitFor(() => expect(loupistIsRunning("Contrarian Value")).toBe(false));
    } finally {
      release();
    }
  });

  it("keeps two Chats of the same Loupist independently live", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const release = bridge.pauseChatStreams();
    try {
      const chatSend = vi.spyOn(bridge, "chatSend");
      const chatAbort = vi.spyOn(bridge, "chatAbort");
      await openApp(bridge);
      await send(user, "First quality thread");
      await screen.findByRole("status", { name: "How should we read the margin decline? is running" });
      await user.click(screen.getByRole("button", { name: /^Capital allocation checklist$/ }));
      await send(user, "Second quality thread");
      await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(2));
      expect(screen.getByRole("status", { name: "Capital allocation checklist is running" })).toBeVisible();
      const [first, second] = await Promise.all(chatSend.mock.results.map((result) => result.value));
      await user.click(screen.getByRole("button", { name: /^How should we read the margin decline\?$/ }));
      await user.click(await screen.findByRole("button", { name: "Stop response" }));
      await waitFor(() => expect(chatAbort).toHaveBeenCalledWith(first.run_id));
      expect(chatAbort).not.toHaveBeenCalledWith(second.run_id);
    } finally {
      release();
    }
  });

  it("waits for the canonical aborted message before settling Stop", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 100 });
    await openApp(bridge);
    await send(user, "Persist this stopped turn");
    await user.click(await screen.findByRole("button", { name: "Stop response" }));
    await waitFor(() => expect(document.querySelector("article.message.assistant.aborted")).toBeVisible());
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message Loupist" })).toBeEnabled());
    expect((await bridge.loupistSelect("loupist-quality")).threads[0]?.messages.at(-1)?.status).toBe("aborted");
  });

  it.each(["complete", "error"] as const)("keeps the canonical %s terminal when Stop loses the race", async (status) => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const originalSelect = bridge.loupistSelect.bind(bridge);
    const stale = await originalSelect("loupist-quality");
    const canonical = structuredClone(stale);
    const content = `Canonical ${status} won the Stop race.`;
    canonical.threads[0]!.messages.push(
      { id: "user-race", role: "user", content: "Race the native terminal boundary", created_at: "2026-08-25T00:00:00.000Z", status: "complete" },
      { id: `assistant-race-${status}`, role: "assistant", content, created_at: "2026-08-25T00:00:01.000Z", status },
    );
    let selections = 0;
    vi.spyOn(bridge, "loupistSelect").mockImplementation((loupistId) =>
      loupistId !== "loupist-quality" ? originalSelect(loupistId) : Promise.resolve(++selections === 1 ? stale : canonical));
    vi.spyOn(bridge, "runActivityList").mockResolvedValue([]);
    const chatAbort = vi.spyOn(bridge, "chatAbort").mockResolvedValue();
    vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
      observer({ type: "started", run_id: request.run_id });
      observer({ type: "delta", run_id: request.run_id, text: "Provisional response that must be replaced." });
      return { run_id: request.run_id };
    });
    await openApp(bridge);
    await send(user, "Race the native terminal boundary");
    await screen.findByText("Provisional response that must be replaced.");
    await user.click(screen.getByRole("button", { name: "Stop response" }));
    await waitFor(() => expect(chatAbort).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(content)).toBeVisible();
    expect(screen.queryByText("Provisional response that must be replaced.")).not.toBeInTheDocument();
  });

  it("keeps a background artifact out of another Loupist's workspace", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 35 });
    await openApp(bridge);
    await user.click(newChatButton());
    await send(user, "Background document");
    await waitForLoupistRunning("Quality Compounder");
    await chooseLoupist(user, "Contrarian Value");
    await waitForLoupistRunning("Quality Compounder", false);
    expect(screen.queryByLabelText("Chat workspace panel")).not.toBeInTheDocument();
    await chooseLoupist(user, "Quality Compounder");
    await openChat("Background document");
    await user.click(screen.getByRole("button", { name: /Open document Research note/ }));
    expect(await screen.findByLabelText("Chat workspace panel")).toBeVisible();
  });

  it("merges a late canonical Loupist snapshot without losing a completed local stream", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 35 });
    const originalSelect = bridge.loupistSelect.bind(bridge);
    await openApp(bridge);
    await send(user, "Preserve this response across a stale selection");
    await screen.findByLabelText("Work progress");
    const stale = await originalSelect("loupist-quality");
    stale.threads[0] = { ...stale.threads[0]!, title: "Canonical remote thread title", use_memory: false };
    await chooseLoupist(user, "Contrarian Value");
    await openChat("Downside scenario review");
    let resolveSelection!: (workspace: typeof stale) => void;
    vi.spyOn(bridge, "loupistSelect").mockImplementationOnce(() => new Promise((resolve) => { resolveSelection = resolve; }));
    await chooseLoupist(user, "Quality Compounder");
    await waitFor(async () => {
      const latest = await originalSelect("loupist-quality");
      expect(latest.threads[0]?.messages.some((message) => message.content.includes("Preserve this response"))).toBe(true);
    }, { timeout: 15_000 });
    await act(async () => resolveSelection(stale));
    await openChat("Canonical remote thread title");
    expect(await screen.findByText(/I reviewed.*Preserve this response across a stale selection/)).toBeVisible();
    expect(privateChatToggle()).toHaveAttribute("aria-pressed", "true");
  });

  it("rehydrates completed Chat messages from the native store after remount", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const first = render(<App bridge={bridge} />);
    await openChat("How should we read the margin decline?");
    await send(user, "Persist this answer through an app remount");
    await screen.findByText("Response complete.");
    first.unmount();
    render(<App bridge={bridge} />);
    await openChat();
    expect(await screen.findByText(/I reviewed.*Persist this answer through an app remount/)).toBeVisible();
    const digest = screen.getByLabelText("Work progress");
    expect(digest).toHaveAttribute("data-expanded", "false");
    await user.click(within(digest).getByRole("button"));
    expect(digest).toHaveTextContent("Searched Memory");
    expect(digest).toHaveTextContent("Read saved notes");
  });
});
