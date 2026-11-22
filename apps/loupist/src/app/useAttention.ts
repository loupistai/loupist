import { useEffect, useRef, useState } from "react";
import i18n from "@/i18n";
import type { AttentionItem } from "../contracts/attention";
import type { ChatThread, LoupistSummary, LoupistBridge } from "../types";
import { CONFIG_CHANGED_EVENT } from "./useConfiguration";
import { isEmptyChatThreadId } from "./emptyChat";

export type AttentionRow = AttentionItem & { taskKind?: string };

const REFRESH_MS = 60_000;

/**
 * The cross-Loupist attention list from Rust, re-read when the window regains
 * focus, when scheduled results change, when a run ends (`refreshKey`) and
 * once a minute so due checks roll over.
 */
export function useAttention(bridge: LoupistBridge, refreshKey: string, threads: Readonly<Record<string, readonly ChatThread[] | undefined>>) {
  const [items, setItems] = useState<AttentionRow[]>([]);
  const loaded = useRef(threads);
  loaded.current = threads;
  useEffect(() => {
    let generation = 0;
    const read = async () => {
      const request = ++generation;
      try {
        const next = await bridge.attentionList();
        if (generation !== request) return;
        // An unopened profile has no chat snapshot. Its built-in briefing owns a dedicated session.
        const owners = new Set(next.filter(item => item.kind === "question" && item.chatId
          && !loaded.current[item.loupistId]?.some(thread => thread.id === item.chatId && thread.messages.length > 0)).map(item => item.loupistId));
        const briefings = new Map(await Promise.all([...owners].map(async owner => {
          const settings = await bridge.briefingSettingsGet({ loupistId: owner }).catch(() => null);
          return [owner, settings?.task] as const;
        })));
        if (generation !== request) return;
        setItems(next.map(item => {
          const task = briefings.get(item.loupistId);
          return item.kind === "question" && item.chatId && task?.session_thread_id === item.chatId
            ? { ...item, taskKind: task.kind } : item;
        }));
      } catch { /* Keep the last attention list while the host is unavailable. */ }
    };
    read();
    window.addEventListener("focus", read);
    window.addEventListener(CONFIG_CHANGED_EVENT, read);
    const unsubscribe = bridge.onInboxChanged(read);
    const timer = window.setInterval(read, REFRESH_MS);
    return () => {
      generation++;
      window.removeEventListener("focus", read);
      window.removeEventListener(CONFIG_CHANGED_EVENT, read);
      unsubscribe();
      window.clearInterval(timer);
    };
  }, [bridge, refreshKey]);
  return items;
}

/** A visible window behind another app still shows its chat; only a hidden one does not. */
const windowVisible = () => document.visibilityState === "visible";

/**
 * Chats whose run ended while the user was not looking at them (another chat,
 * Loupist or page, or a hidden window). Each gets a grey dot until opened, and an
 * OS notification when the user allowed them; Rust writes the body and rate-limits.
 */
export function useFinishedWhileHidden({ bridge, loupists, runningThreadKeys, visibleChatKey }: {
  bridge: LoupistBridge;
  loupists: readonly LoupistSummary[];
  runningThreadKeys: ReadonlySet<string>;
  /** The chat on screen right now, or null when the Chat tab is not showing one. */
  visibleChatKey: string | null;
}) {
  const [finished, setFinished] = useState<ReadonlySet<string>>(() => new Set());
  const previous = useRef<ReadonlySet<string>>(runningThreadKeys);
  const visible = useRef(visibleChatKey);
  visible.current = visibleChatKey;
  const loupistNames = useRef(new Map<string, string>());
  loupistNames.current = new Map(loupists.map((loupist) => [loupist.id, loupist.name]));

  useEffect(() => {
    const ended = [...previous.current].filter((key) => !runningThreadKeys.has(key));
    previous.current = runningThreadKeys;
    // A new chat's run starts under the draft id and moves to the real one; that is not an ending.
    const unseen = ended.filter((key) => !isEmptyChatThreadId(splitChatKey(key).chatId) && (key !== visible.current || !windowVisible()));
    if (!unseen.length) return;
    setFinished((current) => new Set([...current, ...unseen]));
    for (const key of unseen) {
      const { loupistId, chatId } = splitChatKey(key);
      const loupist = loupistId ? loupistNames.current.get(loupistId) : undefined;
      if (!loupistId || !chatId || !loupist) continue;
      void bridge.attentionNotify({ loupistId, chatId, title: i18n.t("app:notifications.finished", { loupist }) }).catch(() => undefined);
    }
  }, [bridge, runningThreadKeys]);

  // Opening the chat while the window is visible clears its dot.
  useEffect(() => {
    if (!visibleChatKey) return;
    const clear = () => {
      if (!windowVisible()) return;
      setFinished((current) => {
        if (!current.has(visibleChatKey)) return current;
        const next = new Set(current);
        next.delete(visibleChatKey);
        return next;
      });
    };
    clear();
    window.addEventListener("focus", clear);
    document.addEventListener("visibilitychange", clear);
    return () => { window.removeEventListener("focus", clear); document.removeEventListener("visibilitychange", clear); };
  }, [visibleChatKey]);

  return { finishedKeys: finished };
}

/** The inverse of `chatSessionKey`. */
export function splitChatKey(key: string) {
  const [loupist = "", chat = ""] = key.split(":");
  return { loupistId: decodeURIComponent(loupist), chatId: decodeURIComponent(chat) };
}
