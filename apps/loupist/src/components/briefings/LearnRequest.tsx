import { createContext, useContext, useId } from "react";
import { useTranslation } from "@/i18n";
import { hasAiAccount } from "@/lib/modelSelection";
import type { ModelCatalog } from "../../types";
import { Button } from "../ui/button";
import "@/styles/briefings.css";

/** Why a Learn entry point cannot ask Bot now; `loading` while the account or settings are still being read. */
export type LearnBlock = "noAccount" | "memoryOff" | "loading" | null;

/**
 * One function decides whether Bot can be asked to set up learning: a learning program
 * needs an AI account and Memory reading and saving on (plan 4.4).
 */
export function learnBlock(catalog: ModelCatalog | null | undefined, memory: { use_memory: boolean; update_memory: boolean } | null | undefined): LearnBlock {
  if (!catalog || !memory) return "loading";
  if (!hasAiAccount(catalog)) return "noAccount";
  return memory.use_memory && memory.update_memory ? null : "memoryOff";
}

type LearnRequests = { block: LearnBlock; ask: (text: string) => void };
const LearnRequestContext = createContext<LearnRequests | null>(null);
export const LearnRequestProvider = LearnRequestContext.Provider;

/**
 * "Learn about this" (World inspector) and "Keep this page current" (a Lens subject page):
 * sends Bot a request that names the subject and opens Bot, where the proposal card
 * arrives (plan 4.4). Disabled, with the reason under it, without an AI account or with
 * Memory saving off. Absent where no app provides the request.
 */
export function LearnRequest({ kind, subject }: { kind: "world" | "lens"; subject: string }) {
  const { t } = useTranslation("briefings");
  const requests = useContext(LearnRequestContext);
  const reasonId = useId();
  if (!requests) return null;
  const { block, ask } = requests;
  const reason = block === "noAccount" ? t("schedule.learning.entry.noAccount") : block === "memoryOff" ? t("schedule.learning.entry.memoryOff") : null;
  const request = kind === "world" ? t("schedule.learning.entry.requestWorld", { name: subject }) : t("schedule.learning.entry.requestLens", { title: subject });
  return <div className="learn-request" data-learn-request={kind}>
    <Button variant="outline" size="sm" disabled={block !== null} aria-describedby={reason ? reasonId : undefined} onClick={() => ask(request)}>
      {t(kind === "world" ? "schedule.learning.entry.learnAbout" : "schedule.learning.entry.keepCurrent")}
    </Button>
    {reason ? <p id={reasonId} className="learn-request-reason">{reason}</p> : null}
  </div>;
}
