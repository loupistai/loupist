import { createMockBridgeState } from "./state";
import { chatArtifactRead, chatArtifactRestore, chatArtifactVersions, chatCreate, chatSend, dismissHandoffLink, ensureHomeThread, researchHandoffStart } from "./loupistChat";
import { initialModelCatalog } from "./models";

it("retains historical content and restores it as a new version without replacing either answer", async () => {
  const state = createMockBridgeState(0);
  const [loupistId, threads] = Object.entries(state.threads)[0]!;
  const threadId = threads[0]!.id;
  const entry = {
    artifact: { id: "note", chat_session_id: threadId, kind: "markdown" as const, title: "Original", current_revision: 1, created_at_ms: 1, updated_at_ms: 1 },
    content: { artifact_id: "note", revision: 1, digest: "a".repeat(64), source_message_id: "answer-1", created_at_ms: 1,
      payload: { kind: "markdown" as const, schema: "loupist-markdown/1" as const, markdown: "Original text" } },
  };
  state.artifacts[threadId] = [entry];
  await chatArtifactRead(state, loupistId, threadId, "note");
  entry.artifact = { ...entry.artifact, title: "Revised", current_revision: 2, updated_at_ms: 2 };
  entry.content = { ...entry.content, revision: 2, digest: "b".repeat(64), source_message_id: "answer-2", created_at_ms: 2, payload: { ...entry.content.payload, markdown: "Revised text" } };
  const original = await chatArtifactRead(state, loupistId, threadId, "note", 1);
  expect(original.revision.payload).toMatchObject({ markdown: "Original text" });
  expect(original.artifact).toMatchObject({ title: "Original", current_revision: 2 });
  const request = { loupist_id: loupistId, thread_id: threadId, artifact_id: "note", revision_id: 1, expected_current_revision: 2 };
  const restored = await chatArtifactRestore(state, request);
  expect(restored.revision).toMatchObject({ revision: 3, source_message_id: "answer-1", payload: { markdown: "Original text" } });
  expect((await chatArtifactVersions(state, loupistId, threadId, "note"))).toMatchObject([
    { revision_id: 3, restored_from_revision_id: 1 }, { revision_id: 2 }, { revision_id: 1 },
  ]);
  expect((await chatArtifactRead(state, loupistId, threadId, "note", 2)).revision.payload).toMatchObject({ markdown: "Revised text" });
  await expect(chatArtifactRestore(state, request)).rejects.toThrow("changed before restore");
  await expect(chatArtifactRead(state, "another-loupist", threadId, "note", 1)).rejects.toThrow("Chat thread not found");
  for (let revision = 4; revision <= 23; revision += 1) {
    entry.artifact = { ...entry.artifact, current_revision: revision };
    entry.content = { ...entry.content, revision };
    await chatArtifactRead(state, loupistId, threadId, "note");
  }
  const retained = await chatArtifactVersions(state, loupistId, threadId, "note");
  expect(retained).toHaveLength(20);
  expect(retained.at(-1)?.revision_id).toBe(4);
  await expect(chatArtifactRead(state, loupistId, threadId, "note", 1)).rejects.toThrow("no longer available");
});


it("scopes retained hand-off links and preserves the first accepted settings across retries", async () => {
  const state = createMockBridgeState(0);
  const loupistId = state.loupists[0]!.id;
  const home = ensureHomeThread(state, loupistId);
  const link = {
    connectionId: "link-owned", seedName: "Alpha", farName: "Beta",
    nodeIds: ["a", "b"], path: [{ name: "Beta", relation: "SUPPLIES" }],
  };
  for (const connectionId of ["link-owned", "link-dismissed"]) {
    home.messages.push({ id: connectionId, role: "assistant", status: "complete", host_composed: true,
      content: "Stored link.", created_at: "2026-11-01T00:00:00Z", link: { ...link, connectionId } });
  }
  home.messages.push({ id: "link-pending", role: "assistant", status: "streaming", host_composed: true,
    content: "Pending link.", created_at: "2026-11-01T00:00:00Z", link: { ...link, connectionId: "link-pending" } });
  const catalog = initialModelCatalog();
  const defaults = () => ({ model_profile_id: null, thinking_level: null, run_options: {}, use_memory: true, update_memory: true });
  const request = {
    loupistId, connectionId: "link-owned", modelProfileId: "model-test", thinkingLevel: "low",
    runOptions: { performance: "fast" }, useMemory: false, updateMemory: false,
  };
  const start = (intent = request) => researchHandoffStart(state, intent, catalog, defaults);
  dismissHandoffLink(state, loupistId, "link-dismissed");
  await expect(start({ ...request, connectionId: "link-dismissed" })).rejects.toMatchObject({ code: "not_found" });
  await expect(start({ ...request, connectionId: "link-pending" })).rejects.toMatchObject({ code: "not_found" });
  await expect(start({ ...request, loupistId: state.loupists[1]!.id })).rejects.toMatchObject({ code: "not_found" });
  await expect(start({ ...request, thinkingLevel: "unavailable" })).rejects.toMatchObject({ code: "invalid_request" });
  const count = state.threads[loupistId]!.length;
  const firstStart = start();
  const retryStart = start();
  // At work, the session carries its run and Bot posts nothing for it.
  expect(state.threads[loupistId]![0]!.handoff_run).toMatchObject({ connection_id: "link-owned" });
  expect(home.messages.some((message) => message.handoff)).toBe(false);
  await expect(start({ ...request, updateMemory: true })).rejects.toMatchObject({ code: "conflict" });
  const [first, retry] = await Promise.all([firstStart, retryStart]);
  expect(retry).toEqual(first);
  expect(home.messages.filter((message) => message.handoff).map((message) => message.handoff))
    .toEqual([{ handoffId: first.handoffId, chatId: first.chatId, connectionId: "link-owned", title: "Link: Alpha and Beta", status: "done" }]);
  expect(state.threads[loupistId]![0]!.handoff_run).toBeUndefined();
  expect(state.threads[loupistId]).toHaveLength(count + 1);
  expect(state.handoffLinks.get(JSON.stringify([loupistId, "link-owned"]))?.status).toBe("opened");
  expect(state.threads[loupistId]?.find((thread) => thread.id === first.chatId)).toMatchObject({
    use_memory: false, update_memory: false, messages: [
      {
        handoff_action: { kind: "link", connectionId: "link-owned" },
        handoff_note: { title: "Link: Alpha and Beta", linkPath: {
          seed: "Alpha", far: "Beta", path: [{ from: "Alpha", relation: "SUPPLIES", to: "Beta" }],
        } },
      },
      { execution_model: { profile_id: "model-test", thinking_level: "low", run_options: { performance: "fast" } } },
    ],
  });
  expect(await start({ ...request, updateMemory: true })).not.toEqual(first);
  expect(state.threads[loupistId]).toHaveLength(count + 2);
});

it.each([
  ["Compare the annual revenue and operating margins for both issuers", "Compare the annual revenue and…"],
  ["  First   line\nMore context", "First line"],
  ["x".repeat(40), "x".repeat(40)],
  ["가".repeat(38) + "👨‍👩‍👧‍👦조사", "가".repeat(38) + "…"],
])("names a preview chat from the first prompt with the host's readable boundary", async (prompt, expected) => {
  const state = createMockBridgeState(0);
  const loupistId = state.loupists[0]!.id;
  const thread = await chatCreate(state, { loupist_id: loupistId });
  const model = { profile_id: "test", provider: "test", model: "test", name: "Test", thinking_level: "medium", run_options: {} };
  const title = vi.fn();
  await new Promise<void>((resolve, reject) => {
    void chatSend(state, { run_id: "run-title", loupist_id: loupistId, thread_id: thread.id, prompt, model_profile_id: "test",
      thinking_level: "medium", run_options: {}, use_memory: false, update_memory: false, attachments: [] }, event => {
      if (event.type === "title") title(event.title);
      if (event.type === "completed") resolve();
      if (event.type === "error") reject(new Error(event.message));
    }, [], model).catch(reject);
  });
  expect(title).toHaveBeenCalledExactlyOnceWith(expected);
});
