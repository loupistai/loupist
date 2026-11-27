import { useEffect, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatDateTime, formatNumber } from "@/i18n/format";
import type { LearningWaitReason } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { Button } from "../ui/button";

/** One sentence per reason method learning waits; the host decides the reason. */
export function learningWaitText(t: (key: string, options?: Record<string, unknown>) => string, reason: LearningWaitReason): string {
  switch (reason.kind) {
    case "off": return t("learningState.off");
    case "no_second_case": return t("learningState.noSecondCase");
    case "subjects_pending": return t("learningState.subjectsPending", { count: reason.n, formattedCount: formatNumber(reason.n) });
    case "sources_unread": return t("learningState.sourcesUnread", { count: reason.n, formattedCount: formatNumber(reason.n) });
    case "rate_limited": return t("learningState.rateLimited", { time: formatDateTime(reason.untilMs, { year: undefined }) });
    case "allowance": return t("learningState.allowance");
  }
}

/** Why method learning waits, in one line of the Lens learning panel; nothing while it does not. */
export function LearningWaitLine({ bridge, loupistId }: { bridge: LoupistBridge; loupistId: string }) {
  const { t } = useTranslation("knowledge");
  const [reason, setReason] = useState<LearningWaitReason | null | "error">(null);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let live = true;
    setReason(null);
    bridge.knowledgeLearningState({ loupistId }).then(state => { if (live) setReason(state.reason); }, () => { if (live) setReason("error"); });
    return () => { live = false; };
  }, [bridge, loupistId, attempt]);
  if (!reason) return null;
  if (reason === "error") return <p className="learning-meta" role="alert" data-learning-wait="error">{t("learningState.loadError")} <Button variant="link" size="sm" onClick={() => setAttempt(value => value + 1)}>{t("map.retry")}</Button></p>;
  return <p className="learning-meta" role="status" data-learning-wait={reason.kind}>{learningWaitText(t, reason)}</p>;
}
