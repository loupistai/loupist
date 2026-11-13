import { readUIMessageStream } from "ai";
import { applyChatProgressPatch, applyChatProgressSnapshot, fromLoupistUIMessage, TauriChatTransport, toLoupistUIMessage } from "./ai-sdk";
import type { LoupistUIMessage } from "./ai-sdk";
import type { ChatMessage, ChatSendRequest, ChatStreamEvent, LoupistBridge } from "../types";
import { evidenceFixture } from "../test/evidenceFixtures";
import { deriveTurnView } from "./turnView";
import { completedRun } from "../test/streamFixtures";

describe("incremental chat progress", () => {
  const tool = { id: "tool-1", kind: "tool" as const, toolName: "web_search", category: "web" as const, operation: "search" as const, action: "Searched the web", target: null, href: null, status: "running" as const, startedAtMs: 10, finishedAtMs: null };
  const initial = { sequence: 0, progress: { startedAtMs: 10, finishedAtMs: null, items: [tool] } };

  it("applies ordered upserts, removals, and the terminal finish", () => {
    const updated = applyChatProgressPatch(initial, { sequence: 1, upsertItems: [{ ...tool, status: "succeeded" }, { id: "commentary-2", kind: "commentary", text: "Checked it." }] });
    const finished = applyChatProgressPatch(updated, { sequence: 2, removeItemIds: ["commentary-2"], finishedAtMs: 20 });
    expect(finished).toEqual({ sequence: 2, progress: { startedAtMs: 10, finishedAtMs: 20, items: [{ ...tool, status: "succeeded" }] } });
  });

  it("appends published commentary in place and waits for a snapshot when the id is unknown", () => {
    const note = { id: "commentary-2", kind: "commentary" as const, text: "Checked " };
    const published = applyChatProgressPatch(initial, { sequence: 1, upsertItems: [note] });
    const appended = applyChatProgressPatch(published, { sequence: 2, appendText: [{ id: note.id, text: "the " }, { id: note.id, text: "filing." }] });
    expect(appended?.progress.items).toEqual([tool, { ...note, text: "Checked the filing." }]);
    expect(appended?.progress.items[0]).toBe(tool);
    expect(applyChatProgressPatch(appended, { sequence: 3, appendText: [{ id: "commentary-9", text: "lost" }] })).toBe(appended);
  });

  it("replaces the plan only when a patch carries one", () => {
    const plan = { items: [{ id: "read", label: "Read the report", status: "active" as const, activatedAtMs: 11 }, { id: "compare", label: "Compare", status: "pending" as const }] };
    const planned = applyChatProgressPatch(initial, { sequence: 1, plan });
    expect(planned?.progress.plan).toEqual(plan);
    const kept = applyChatProgressPatch(planned, { sequence: 2, upsertItems: [{ ...tool, status: "succeeded" }] });
    expect(kept?.progress.plan).toBe(plan);
  });

  it("ignores stale and gapped patches until a later full snapshot", () => {
    expect(applyChatProgressPatch(initial, { sequence: 2, finishedAtMs: 20 })).toBe(initial);
    expect(applyChatProgressPatch(initial, { sequence: 0, removeItemIds: ["tool-1"] })).toBe(initial);
    const replacement = { startedAtMs: 10, finishedAtMs: 30, items: [] };
    expect(applyChatProgressSnapshot(initial, replacement, 3)).toEqual({ sequence: 3, progress: replacement });
    expect(applyChatProgressSnapshot(initial, replacement, 0)).toBe(initial);
  });
});

describe("Loupist chat UI message conversion", () => {
  it("keeps live commentary and provider reasoning out of the answer", () => {
    const projected = fromLoupistUIMessage({
      id: "assistant-live", role: "assistant", metadata: { status: "streaming" },
      parts: [
        { type: "text", text: "", state: "streaming" },
        { type: "reasoning", text: "Writing the answer", state: "streaming" },
        { type: "data-progress", id: "progress-live", data: { startedAtMs: 1, items: [{ id: "commentary-1", kind: "commentary", text: "Draft answer text" }], finishedAtMs: null } },
      ],
    });
    expect(projected.content).toBe("");
    expect(projected.progress?.items).toEqual([{ id: "commentary-1", kind: "commentary", text: "Draft answer text" }]);
  });

  it("uses a canonical error terminal instead of streamed partial text", () => {
    const projected = fromLoupistUIMessage({
      id: "assistant-local", role: "assistant",
      metadata: { native_message_id: "assistant-canonical", created_at: "2026-08-09T00:00:01.000Z", status: "error", final_text: "Response could not be completed." },
      parts: [{ type: "text", text: "Partial provider response that must not survive.", state: "done" }],
    });
    expect(projected).toMatchObject({ id: "assistant-canonical", status: "error", content: "Response could not be completed." });
  });

  it("round-trips every persisted answer receipt", () => {
    const message: ChatMessage = {
      id: "assistant-1", role: "assistant", content: "Done", created_at: "2026-08-09T00:00:00.000Z", status: "error",
      failure_kind: "memory_commit_failed", host_composed: true,
      progress: { startedAtMs: 1, finishedAtMs: 2, items: [{ id: "tool-2", kind: "tool", category: "memory", operation: "read", action: "Read Memory", status: "succeeded", target: null, href: null, startedAtMs: 0, finishedAtMs: null }] },
      artifact_refs: [{ artifact_id: "artifact-1", revision: 2, kind: "chart", title: "Price history", digest: "a".repeat(64) }],
      attachments: [{ id: "attachment-1", filename: "chart.png", media_type: "image/png", size_bytes: 42 }],
      agent_harness: { schema: "loupist-harness/1", mode: "chat", skill_ids: ["research"], capability_ids: ["world-bank.indicators"], digest: "e".repeat(64) },
      performance: { setupMs: 120, firstTextMs: 480, generationMs: 900, totalMs: 940, sessionCache: "warm", inputTokens: 320, outputTokens: 96, cacheReadTokens: 256, cacheWriteTokens: 64 },
      memory_refs: [{ read_ref: "journal-1", kind: "journal", title: "Capacity review", excerpt: "", access: "exact_read", knowledge: { kind: "journal", recordId: "private", revisionId: "sealed-1", objectId: "journal-1" }, as_of: null, section: null }],
      memory_update: { status: "applied", commitId: "commit-1", changes: [{ recordId: "journal-1", kind: "Journal", operation: "create", title: "Capacity review", knowledge: { kind: "journal", recordId: "", revisionId: "", objectId: "journal-1" }, lesson: "Review recorded", basis: "Current sources", futureUse: "Compare subsequent findings" }] },
      schedule_receipts: [{ task_id: "scheduled-a", title: "Daily review", revision: 1, action: "created", status: "active", schedule: { kind: "interval", every_minutes: 60 }, target: { kind: "dedicated_chat" }, next_run_at_ms: 123456, use_memory: false, update_memory: false, awaiting_confirmation: false }],
      tool_refusals: [{ connection_id: "custom-one", connection_name: "Company research", tool_name: "read_filings", reason: "needs_approval" }],
    };
    expect(fromLoupistUIMessage(toLoupistUIMessage(message))).toMatchObject(message);
  });
});

describe("native stream transport", () => {
  const body = { loupist_id: "loupist-a", thread_id: "chat-a", privateChat: true, model_profile_id: "model-a", thinking_level: "medium", run_options: {} };
  const request = {
    trigger: "submit-message" as const, chatId: "chat-a", messageId: undefined, abortSignal: undefined,
    messages: [{ id: "user-1", role: "user" as const, parts: [{ type: "text" as const, text: "Read this." }] }],
  };
  const transport = (chatSend: (sent: ChatSendRequest, onEvent: (event: ChatStreamEvent) => void) => Promise<{ run_id: string }>) =>
    new TauriChatTransport({ chatSend: vi.fn(chatSend), chatAbort: vi.fn() } as unknown as LoupistBridge);
  const project = async (stream: ReadableStream<never>) => {
    const projections: ChatMessage[] = [];
    for await (const message of readUIMessageStream<LoupistUIMessage>({ stream })) projections.push(fromLoupistUIMessage(message));
    return projections;
  };

  it("sends the same follow-up request with or without the prior answer's activity and source receipts", async () => {
    const answer: ChatMessage = { id: "prior", role: "assistant", content: "Revenue increased.", created_at: "2026-10-01T12:00:00Z" };
    const receipt = evidenceFixture({ title: "Quarterly report" });
    const withActivity: ChatMessage = {
      ...answer, evidence_refs: [receipt],
      progress: { startedAtMs: 1, finishedAtMs: 3, items: [
        { kind: "commentary", id: "note", text: "Comparing quarterly reports." },
        { kind: "tool", id: "read", category: "files", operation: "read", action: "Read a filing",
          target: "filing.pdf", detailId: "local-detail", resultIds: [receipt.id], status: "succeeded", href: null, startedAtMs: 0, finishedAtMs: null },
      ] },
    };
    const sent: ChatSendRequest[] = [];
    const chat = transport(async (args, emit) => {
      sent.push(args);
      emit({ type: "aborted", run_id: args.run_id });
      return { run_id: args.run_id };
    });
    for (const prior of [answer, withActivity]) {
      const stream = await chat.sendMessages({ ...request, body, messages: [toLoupistUIMessage(prior), ...request.messages] });
      await project(stream as ReadableStream<never>);
    }
    expect(sent).toHaveLength(2);
    expect(sent[0]?.prompt).toBe("Read this.");
    expect(sent[1]).toEqual({ ...sent[0], run_id: expect.any(String) });
  });

  it("shows one answer under one key in every projection while a draft settles, then a second turn joins it", async () => {
    const preamble = { kind: "commentary" as const, id: "note", text: "Checking." };
    const draft = { kind: "commentary" as const, id: "draft", text: "Finished." };
    const second = { kind: "commentary" as const, id: "second", text: "Then" };
    const stream = await transport(async ({ run_id }, emit) => {
      emit({ type: "started", run_id });
      emit({ type: "phase", phase: "writing", run_id });
      emit({ type: "progress", sequence: 1, progress: { startedAtMs: 1, items: [preamble, draft], finishedAtMs: null }, run_id });
      emit({ type: "delta", text: draft.text, settles: draft.id, run_id });
      emit({ type: "progress_patch", patch: { sequence: 2, removeItemIds: [draft.id] }, run_id });
      emit({ type: "progress_patch", patch: { sequence: 3, upsertItems: [second] }, run_id });
      emit({ type: "progress_patch", patch: { sequence: 4, appendText: [{ id: second.id, text: " more." }] }, run_id });
      emit({ type: "delta", text: "\n\nThen more.", settles: second.id, run_id });
      emit({ type: "aborted", run_id });
      return { run_id };
    }).sendMessages({ ...request, body });
    const projections = await project(stream as ReadableStream<never>);
    const answers = projections.map((message) => {
      const turn = deriveTurnView({ message, now: 2, lastActivityAt: 2, stopping: false, retry: { attempt: 1, offered: false } });
      const prose = turn.segments.filter((segment) => segment.kind === "prose");
      // A note stays a note: only the draft and its settled text read as the answer.
      if (message.progress) expect(prose.filter((segment) => segment.tone === "note").map((segment) => segment.text)).toEqual(["Checking."]);
      return prose.filter((segment) => segment.tone === "answer");
    });
    expect(answers.every((segments) => segments.length <= 1 && segments.every((segment) => segment.key === draft.id))).toBe(true);
    const texts = answers.flatMap((segments) => segments.map((segment) => segment.text)).filter((text, index, all) => text !== all[index - 1]);
    expect(texts).toEqual(["Finished.", "Finished.\n\nThen", "Finished.\n\nThen more."]);
    expect(projections.at(-1)).toMatchObject({ answer_key: draft.id, draft: { id: second.id, answerLength: draft.text.length, settled: true } });
    expect(projections.at(-1)?.progress?.items).toEqual([preamble]);
  });

  it("keeps narration on the progress channel and publishes a timestamped streaming assistant first", async () => {
    const createdAt = "2026-10-11T03:55:00.000Z";
    vi.useFakeTimers();
    vi.setSystemTime(new Date(createdAt));
    try {
      const stream = await transport(async (sent, onEvent) => {
        onEvent({ type: "started", run_id: sent.run_id! });
        onEvent({ type: "progress", run_id: sent.run_id!, sequence: 1, progress: { startedAtMs: 1, items: [{ id: "note", kind: "commentary", text: "I will check the filing." }], finishedAtMs: null } });
        onEvent({ type: "aborted", run_id: sent.run_id! });
        return { run_id: sent.run_id! };
      }).sendMessages({ ...request, body });
      const projections = await project(stream as ReadableStream<never>);
      expect(projections[0]).toMatchObject({ role: "assistant", content: "", created_at: createdAt, status: "streaming" });
      expect(projections.at(-1)).toMatchObject({ created_at: createdAt, status: "aborted", content: "Response stopped." });
      expect(projections.at(-1)!.progress?.items).toEqual([{ id: "note", kind: "commentary", text: "I will check the filing." }]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("ignores a stream event type this build does not know and finishes the reply", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      const stream = await transport(async (sent, onEvent) => {
        const run_id = sent.run_id!;
        onEvent({ type: "started", run_id });
        onEvent({ type: "from_a_newer_host", run_id, message: "not an error" } as unknown as ChatStreamEvent);
        onEvent({ type: "delta", run_id, text: "Still here." });
        onEvent({ type: "completed", ...completedRun, run_id, message_id: "after-unknown", final_text: "Still here.", created_at: "2026-11-01T00:00:00Z",
          execution_model: { profile_id: "test", name: "Test", provider: "test", model: "test", thinking_level: "medium", run_options: {} },
          agent_harness: { schema: "test", mode: "chat", skill_ids: [], capability_ids: [], digest: "test" } });
        return { run_id };
      }).sendMessages({ ...request, body });
      const projections = await project(stream as ReadableStream<never>);
      expect(projections.at(-1)).toMatchObject({ status: "complete", content: "Still here." });
      expect(warn).toHaveBeenCalledWith(expect.stringContaining("from_a_newer_host"));
    } finally {
      warn.mockRestore();
    }
  });

  it.each([false, true])("reports admission and sends only paired Memory flags to IPC (private: %s)", async (privateChat) => {
    const onAdmission = vi.fn();
    const chatSend = vi.fn(async (sent: ChatSendRequest) => ({ run_id: sent.run_id! }));
    const stream = await new TauriChatTransport({ chatSend } as unknown as LoupistBridge).sendMessages({ ...request, body: { ...body, privateChat, onAdmission } });
    await Promise.resolve();
    const sent = chatSend.mock.calls[0]![0];
    expect(onAdmission).toHaveBeenCalledWith({ accepted: true, runId: sent.run_id });
    expect(sent).not.toHaveProperty("onAdmission");
    expect(sent).not.toHaveProperty("privateChat");
    expect(sent).toMatchObject({ use_memory: !privateChat, update_memory: !privateChat });
    await stream.cancel();
  });

  it("rejects an unaccepted start without fabricating an answer", async () => {
    const onAdmission = vi.fn();
    const stream = await transport(async () => { throw new Error("Start denied"); }).sendMessages({ ...request, body: { ...body, onAdmission } });
    await expect(stream.getReader().read()).rejects.toThrow("Start denied");
    expect(onAdmission).toHaveBeenCalledExactlyOnceWith({ accepted: false, error: "Start denied" });
  });

  it("words Rust's refusal of a file over 25 MB in the reader's language", async () => {
    const onAdmission = vi.fn();
    const refusal = { code: "attachment_too_large", message: "deck.pdf is larger than 25 MB" };
    const stream = await transport(async () => { throw refusal; }).sendMessages({ ...request, body: { ...body, onAdmission } });
    await expect(stream.getReader().read()).rejects.toThrow("Each file must be 25 MB or smaller");
    expect(onAdmission).toHaveBeenCalledExactlyOnceWith({ accepted: false, error: "Each file must be 25 MB or smaller. Remove the larger file and send again." });
  });

  it("keeps an accepted run accepted when a later transport promise fails", async () => {
    const onAdmission = vi.fn();
    let runId = "";
    const stream = await transport(async (sent, onEvent) => {
      runId = sent.run_id!;
      onEvent({ type: "started", run_id: runId });
      throw new Error("Connection ended after admission");
    }).sendMessages({ ...request, body: { ...body, onAdmission } });
    const reader = stream.getReader();
    while (!(await reader.read()).done) { /* Drain the terminal stream error chunk. */ }
    expect(onAdmission).toHaveBeenCalledExactlyOnceWith({ accepted: true, runId });
  });

  it("carries custom tool refusals from stream completion into the answer", async () => {
    const tool_refusals = [{ connection_id: "custom-one", connection_name: "Company research", tool_name: "read_filings", reason: "needs_approval" as const }];
    const stream = await transport(async (sent, onEvent) => {
      const run_id = sent.run_id!;
      onEvent({ type: "started", run_id });
      onEvent({ type: "completed", ...completedRun, run_id, message_id: "custom-refusal", final_text: "Please review this tool.", created_at: "2026-10-28T00:00:00Z", tool_refusals,
        execution_model: { profile_id: "test", name: "Test", provider: "test", model: "test", thinking_level: "medium", run_options: {} },
        agent_harness: { schema: "test", mode: "chat", skill_ids: [], capability_ids: [], digest: "test" } });
      return { run_id };
    }).sendMessages({ ...request, body });
    expect((await project(stream as ReadableStream<never>)).at(-1)!.tool_refusals).toEqual(tool_refusals);
  });
});
