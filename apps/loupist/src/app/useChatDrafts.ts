import { useCallback, useEffect, useRef, useState } from "react";
import type { FileUIPart } from "ai";
import { createChatDraftSession, type ChatDraftSession } from "../chat/draftSession";
import type { KnowledgePin } from "../types";
import { sameKnowledgePin } from "../knowledge/location";
import type { ComposerChip } from "../chat/composerChips";
import type { ComposerInlineReference } from "../chat/composerInlineReferences";

export type ChatDraft = {
  prompt: string;
  pins: KnowledgePin[];
  files: (FileUIPart & { id: string })[];
  onFilesChanged?: (files: ChatDraft["files"]) => void;
  chips?: ComposerChip[];
  /** Plain textarea reference ranges; retained with this draft across view switches. */
  inlineReferences?: ComposerInlineReference[];
  /** The as-of date (`YYYY-MM-DD`); it stays after sending until the user removes it. */
  asOf?: string;
};
export type ChatSessionDrafts = ChatDraftSession;
const emptyDraft = (): ChatDraft => ({ prompt: "", pins: [], files: [], chips: [] });

/** Owns unsent input for the lifetime of the app, including attachment URLs. */
export function useChatDrafts() {
  const drafts = useRef(new Map<string, ChatDraft>());
  const sessions = useRef(new Map<string, ChatSessionDrafts>());
  const getSession = useCallback((loupistId: string) => {
    let session = sessions.current.get(loupistId);
    if (!session) { session = createChatDraftSession(); sessions.current.set(loupistId, session); }
    return session;
  }, []);
  const [, redraw] = useState(0);
  const refresh = useCallback(() => redraw((value) => value + 1), []);
  const get = useCallback((key: string) => {
    let draft = drafts.current.get(key);
    if (!draft) { draft = emptyDraft(); drafts.current.set(key, draft); }
    return draft;
  }, []);
  const adopt = useCallback((from: string, to: string) => {
    const draft = drafts.current.get(from);
    if (draft) { drafts.current.set(to, draft); drafts.current.delete(from); }
    refresh();
  }, [refresh]);
  const addPin = useCallback((draft: ChatDraft, pin: KnowledgePin) => {
    if (!draft.pins.some((existing) => sameKnowledgePin(existing, pin))) draft.pins = [...draft.pins, pin];
    refresh();
  }, [refresh]);
  const removePin = useCallback((draft: ChatDraft, pin: KnowledgePin) => {
    draft.pins = draft.pins.filter((existing) => !sameKnowledgePin(existing, pin));
    refresh();
  }, [refresh]);
  useEffect(() => {
    const owned = drafts.current;
    return () => { for (const draft of owned.values()) for (const file of draft.files) URL.revokeObjectURL(file.url); };
  }, []);
  /** Chat keys whose composer holds unsent text; the sidebar marks them with a dot. */
  const unsentKeys = useCallback(() => {
    const keys = new Set<string>();
    for (const [key, draft] of drafts.current) if (draft.prompt.trim()) keys.add(key);
    return keys;
  }, []);
  return { get, getSession, adopt, addPin, removePin, refresh, unsentKeys };
}
