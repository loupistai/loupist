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
  const first: ChatDraft = { prompt: "Compare Apple", pins: [], files: [] };
  const view = mount(first);
  await waitFor(async () => expect((await bridge.chatDraftRead(id))?.prompt).toBe(first.prompt));
  first.chips = [{ key: "company:AAPL", kind: "company", id: "AAPL:NASDAQ", label: "Apple" }];
  first.inlineReferences = [{ key: "chip:company:AAPL", kind: "company", text: "Apple", start: 8, end: 13 }];
  first.asOf = "2026-03-05";
  view.rerender();
  await waitFor(async () => expect((await bridge.chatDraftRead(id))?.context?.inlineReferences).toEqual(first.inlineReferences));
  view.unmount();

  const next: ChatDraft = { prompt: "", pins: [], files: [] };
  await act(async () => { mount(next); });
  expect(next).toMatchObject(first);
  expect(await bridge.chatDraftRead({ ...id, thread_id: "another-thread" })).toBeNull();
});
