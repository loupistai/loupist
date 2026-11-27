import { setPreviewLanguage } from "@/i18n/useLanguage";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatStreamEvent } from "../../types";
import { evidenceFixture } from "../evidenceFixtures";
import { completedRun } from "../streamFixtures";

it("keeps a finished answer usable while the save waits and preserves it when the user skips saving", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const source = evidenceFixture({ id: "ev_filing", title: "Annual filing", excerpt: "The closing price was $42.10." });
  const answer = "The final traded price was [$42.10](#evidence-ev_filing). [Download the report](outputs/report.md)";
  const skipWarning = "Not saved to Memory: you skipped saving.";
  const title = "Review peer filings";
  const selection = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await selection(id);
    const session = workspace.threads.find(thread => thread.id === "thread-margin");
    if (session) workspace.threads.push({ ...session, id: "chat-blocker", title, messages: [] });
    return workspace;
  });
  let emit: (event: ChatStreamEvent) => void;
  let runId = "";
  const savingStarted = Date.now() - 5_000;
  const numberChecks = [{ text: "$42.10", evidence_id: source.id, link_index: 0, status: "matched", kind: "passage" }] as const;
  const send = vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
    if (!runId) {
      runId = request.run_id;
      emit = observer;
      observer({ type: "started", run_id: runId });
      observer({ type: "phase", run_id: runId, phase: "writing" });
      observer({ type: "evidence", run_id: runId, evidence: source });
      observer({ type: "progress", run_id: runId, sequence: 1, progress: { startedAtMs: Date.now() - 4_000, finishedAtMs: null, items: [
        { id: "write-report", kind: "tool", toolName: "write", category: "files", operation: "write", action: "Wrote a file",
          status: "succeeded", filePath: "outputs/report.md", startedAtMs: Date.now() - 2_000, finishedAtMs: Date.now(), target: null, href: null },
      ] } });
      observer({ type: "delta", run_id: runId, text: "The final traded price was $42.10." });
    }
    return { run_id: request.run_id };
  });
  const stop = vi.spyOn(bridge, "chatAbort").mockImplementation(async () => {
    emit({
      ...completedRun, type: "completed", memory_save_skip: "user", run_id: runId,
      discarded_memory_items: [{ kind: "note", title: "Final trade notes" }],
      message_id: "answer", final_text: answer, created_at: new Date().toISOString(),
      save_activity: { startedAtMs: savingStarted, finishedAtMs: savingStarted + 4_350, waitedMs: 4_300, blockingChatId: "chat-blocker" },
      execution_model: { profile_id: "test", name: "Test", provider: "test", model: "test", thinking_level: "medium", run_options: {} },
      agent_harness: { schema: "test", mode: "chat", skill_ids: [], capability_ids: [], digest: "test" },
    });
  });
  const { unmount } = render(<App bridge={bridge} />);
  const session = await screen.findByRole("button", { name: "How should we read the margin decline?" });
  await user.click(session);
  const composer = screen.getByRole("textbox", { name: "Message Loupist" });
  await user.type(composer, "What was the final traded price?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  await screen.findByRole("button", { name: "Stop response" });
  await waitFor(() => expect(send).toHaveBeenCalledOnce());

  const publishSaving = async (waiting: boolean, blocker?: string) => act(async () => {
    emit({ type: "saving", run_id: runId, waiting_for_other_chat: waiting,
      ...(!waiting ? { final_text: answer, number_checks: [...numberChecks] } : {}),
      ...(blocker ? { blocking_chat_id: blocker } : {}),
      save_activity: { startedAtMs: savingStarted, finishedAtMs: null, waitedMs: blocker ? 4_300 : 0, ...(blocker ? { blockingChatId: blocker } : {}) },
    });
  });
  await publishSaving(false);
  await act(async () => {
    emit({ type: "memory_update", run_id: runId, result: { status: "no_change", commitId: null, changes: [] } });
  });
  const saving = await screen.findByText("Saving to Memory…");
  const status = saving.closest<HTMLElement>('[role="status"]')!;
  const card = within(saving.closest("article")!);
  expect(card.queryByTestId("turn-live")).toBeNull();
  expect(saving.closest("article")!.querySelector("[data-streaming]")).toBeNull();
  expect(card.queryByTestId("memory-row")).toBeNull();
  expect(within(session).getByRole("img", { name: "Saving to Memory" })).toBeVisible();
  expect(session).toHaveAttribute("title", "Saving to Memory");

  try {
    act(() => { setPreviewLanguage("ko"); });
    expect(await screen.findByText("메모리에 저장 중…")).toBeVisible();
    expect(await screen.findByRole("button", { name: "저장 건너뛰기" })).toBeEnabled();
    expect(session).toHaveAttribute("title", "메모리에 저장 중");
    await publishSaving(true);
    await waitFor(() => expect(status).toHaveTextContent("메모리에 저장 중 · 이 Loupist의 다른 대화가 끝나기를 기다리는 중"));
    await publishSaving(true, "chat-blocker");
    await waitFor(() => expect(status).toHaveTextContent(`메모리에 저장 중 · ‘${title}’ 대화가 끝나기를 기다리는 중`));
  } finally {
    act(() => { setPreviewLanguage("en"); });
  }
  await waitFor(() => expect(status).toHaveTextContent(`Saving to Memory · waiting for “${title}” to finish`));
  await publishSaving(true);
  await waitFor(() => expect(status).toHaveTextContent("Saving to Memory · waiting for other conversations in this Loupist"));
  await publishSaving(true, "chat-blocker");
  await user.click(await within(status).findByRole("link", { name: title }));
  expect(await screen.findByRole("heading", { name: title })).toBeVisible();
  await user.click(session);
  const reopened = within((await screen.findByText("The final traded price was", { exact: false })).closest("article")!);
  await user.click(reopened.getByRole("button", { name: "Copy answer" }));
  expect(await navigator.clipboard.readText()).toContain("$42.10");
  await user.click(reopened.getByText("Sources (1)"));
  expect(reopened.getByText("Annual filing")).toBeVisible();
  expect(reopened.getByRole("button", { name: "report.md" })).toBeEnabled();
  expect(reopened.getByTestId("number-chip")).toBeEnabled();
  await user.click(screen.getByTestId("header-panel-toggle"));
  const activity = await screen.findByTestId("activity-sheet");
  expect(within(activity).getByText(/Waited .+ for “Review peer filings”/)).toBeVisible();
  await user.click(within(activity).getByRole("button", { name: "Close activity" }));

  const next = screen.getByRole("textbox", { name: "Message Loupist" });
  await user.type(next, "Compare it with the previous year.{Enter}");
  expect(await screen.findByText("Compare it with the previous year.")).toBeVisible();
  expect(send).toHaveBeenCalledOnce();
  const skip = screen.getByRole("button", { name: "Skip saving" });
  await user.hover(skip);
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Keep the answer and skip saving it to Memory.");
  await user.click(skip);
  expect(await screen.findByText(skipWarning)).toBeVisible();
  expect(screen.getByText("Anything this answer says it saved or filed was not kept.")).toBeVisible();
  expect(screen.getByRole("list", { name: "Not kept" })).toHaveTextContent("Final trade notes");
  expect(screen.getByText("The final traded price was", { exact: false })).toBeVisible();
  expect(stop).toHaveBeenCalledExactlyOnceWith(runId);
  await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
  unmount();
});
