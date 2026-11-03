import { createMockBridgeState } from "./state";
import { chatArtifactRead, chatArtifactRestore, chatArtifactVersions } from "./loupistChat";

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
