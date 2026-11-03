import { createContext, useContext } from "react";
import i18n from "@/i18n";
import type { KnowledgePin } from "../../types";
import { formatAsOf, localDayOf } from "../../chat/asOf";

/** A Journal entry to reread with only what was known when it was written. */
export type LookBackEntry = { loupistId: string; journalId: string; judgment: string; createdAtMs: number };

/**
 * What Look back puts in a new topic Chat's composer (finance-point-in-time.md
 * §2.5): the title, the as-of chip at the entry's local creation day, the
 * entry's journal pin and the question. It never sends.
 */
export function lookBackDraft(entry: LookBackEntry) {
  const asOf = localDayOf(entry.createdAtMs);
  const judgment = entry.judgment.replace(/\s+/gu, " ").trim();
  const short = judgment.length > 40 ? `${judgment.slice(0, 40).trimEnd()}…` : judgment;
  const pin: KnowledgePin = { kind: "journal", loupistId: entry.loupistId, objectId: entry.journalId, label: short };
  return {
    title: i18n.t("knowledge:journal.lookBackTitle", { judgment: short }),
    asOf,
    pin,
    prompt: i18n.t("knowledge:journal.lookBackPrompt", { date: formatAsOf(asOf) }),
  };
}

/** Opens Look back in a new Chat; the app shell provides it. */
export type LookBack = (entry: LookBackEntry) => void;

const LookBackContext = createContext<LookBack | null>(null);
export const LookBackProvider = LookBackContext.Provider;
/** Null outside the shell: the Look back entry points hide themselves. */
export const useLookBack = () => useContext(LookBackContext);
