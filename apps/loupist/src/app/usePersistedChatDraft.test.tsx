import { act, renderHook, waitFor } from "@testing-library/react";
import { useReducer } from "react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import type { ChatDraft } from "./useChatDrafts";
import { usePersistedChatDraft } from "./usePersistedChatDraft";

it("saves context-only edits and restores references with their original prompt and chat", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const writes = { current: Promise.resolve() };
  const id = { loupist_id: "loupist-quality", thread_id: "thread-margin" };
  const select = vi.fn();
  const mount = (draft: ChatDraft) => renderHook(() => {
    const [, refresh] = useReducer(n => n + 1, 0);
    usePersistedChatDraft(bridge, draft, "draft-key", id.loupist_id, id.thread_id, undefined, select, refresh, writes);
  });
  const first: ChatDraft = { prompt: "Read @report", pins: [], files: [] };
  const view = mount(first);
  await waitFor(async () => expect((await bridge.chatDraftRead(id))?.prompt).toBe(first.prompt));
  first.chips = [{ key: "file:report", kind: "file", id: "report.md", label: "report" }];
  first.inlineReferences = [{ key: "chip:file:report", kind: "file", text: "@report", start: 5, end: 12 }];
  first.asOf = "2026-03-05";
  view.rerender();
  await waitFor(async () => expect((await bridge.chatDraftRead(id))?.context?.inlineReferences).toEqual(first.inlineReferences));
  view.unmount();

  const next: ChatDraft = { prompt: "", pins: [], files: [] };
  await act(async () => { mount(next); });
  expect(next).toMatchObject(first);
  expect(await bridge.chatDraftRead({ ...id, thread_id: "another-thread" })).toBeNull();
});
