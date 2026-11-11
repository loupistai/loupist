import { useEffect, useRef, useState } from "react";
import i18n from "@/i18n";
import type { AttentionItem } from "../contracts/attention";
import type { LoupistSummary, LoupistBridge } from "../types";
import { isEmptyChatThreadId } from "./emptyChat";

const REFRESH_MS = 60_000;

/**
 * The cross-Loupist attention list from Rust, re-read when the window regains
 * focus, when scheduled results change, when a run ends (`refreshKey`) and
 * once a minute so due checks roll over.
 */
export function useAttention(bridge: LoupistBridge, refreshKey: string) {
  const [items, setItems] = useState<AttentionItem[]>([]);
  useEffect(() => {
    let live = true;
    const read = () => void bridge.attentionList()
      .then((next) => { if (live) setItems(next); })
      .catch(() => undefined);
    read();
    window.addEventListener("focus", read);
    const unsubscribe = bridge.onInboxChanged(read);
    const timer = window.setInterval(read, REFRESH_MS);
    return () => {
      live = false;
      window.removeEventListener("focus", read);
      unsubscribe();
      window.clearInterval(timer);
    };
  }, [bridge, refreshKey]);
  return items;
}

const windowVisible = () => document.visibilityState === "visible" && document.hasFocus();

/**
 * Chats whose run ended while the user was not looking at them (another chat or
 * Loupist, a hidden or unfocused window). Each gets a grey dot until opened, and an
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
