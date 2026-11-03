import { useEffect, useMemo, useRef, type RefObject } from "react";
import type { ChatArtifactRef, LoupistBridge, StoredChatDraft } from "../types";
import type { ChatDraft } from "./useChatDrafts";

export function usePersistedChatDraft(
  bridge: LoupistBridge,
  draft: ChatDraft | null,
  key: string | null,
  loupistId: string | null,
  threadId: string | null,
  artifact: ChatArtifactRef | undefined,
  selectArtifact: (key: string, artifact?: ChatArtifactRef) => void,
  refresh: () => void,
  writes: RefObject<Promise<void>>,
) {
  const saved = useRef(new Map<string, string>());
  const prompt = draft?.prompt ?? "";
  const pins = draft?.pins;
  const chips = draft?.chips;
  const inlineReferences = draft?.inlineReferences;
  const asOf = draft?.asOf;
  const snapshot = useMemo((): StoredChatDraft => ({
    prompt,
    context: pins?.length || chips?.length || inlineReferences?.length || asOf || artifact
      ? { pins: pins ?? [], chips: chips ?? [], inlineReferences: inlineReferences ?? [], asOf, artifact }
      : undefined,
  }), [prompt, pins, chips, inlineReferences, asOf, artifact]);
  const signature = useMemo(() => JSON.stringify(snapshot), [snapshot]);

  useEffect(() => {
    if (!draft || !key || !loupistId || !threadId) return;
    const identity = { loupist_id: loupistId, thread_id: threadId };
    if (saved.current.get(key) === signature) return;
    if (saved.current.has(key) || snapshot.prompt || snapshot.context) {
      saved.current.set(key, signature);
      writes.current = writes.current.then(() => bridge.chatDraftWrite(identity, snapshot)).catch(() => {
        if (saved.current.get(key) === signature) saved.current.delete(key);
      });
      return;
    }

    let current = true;
    void writes.current.then(() => bridge.chatDraftRead(identity)).then(stored => {
      if (!current || saved.current.has(key)) return;
      saved.current.set(key, JSON.stringify(stored ?? snapshot));
      if (!stored) return;
      draft.prompt = stored.prompt;
      const context = stored.context;
      if (context) {
        draft.pins = context.pins.filter(pin => pin.loupistId === loupistId);
        draft.chips = context.chips;
        draft.inlineReferences = context.inlineReferences;
        draft.asOf = context.asOf;
        if (context.artifact) selectArtifact(key, context.artifact);
      }
      refresh();
    }).catch(() => {});
    return () => { current = false; };
  }, [bridge, draft, key, loupistId, threadId, refresh, selectArtifact, writes, signature, snapshot]);
}
