import i18n, { useTranslation } from "@/i18n";
import { formatTime } from "@/i18n/format";
import { useState } from "react";
import type { ChatEvidenceRef, ChatMessage, LoupistBridge } from "../../types";
import type { AssetEvidence } from "../../contracts/assets";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";
import { useGo } from "../navigation/Open";
import { hostSourceDestination } from "../../chat/navigation";
import { useAssetNames } from "../assets/assetName";
import { EarningsMiniTable } from "../assets/EarningsCard";
import "@/styles/watch.css";
import "@/styles/earnings.css";

const t = i18n.getFixedT(null, "assets");

/**
 * A results message Rust posted when the after-capture committed: "{name}
 * reported {period} results." with the two most material measures; or a
 * before-capture that failed, with Try again. Successful before-captures post
 * nothing.
 */
export function ThreadEarnings({ message, bridge, loupistId, loupistName }: {
  message: ChatMessage;
  bridge?: LoupistBridge;
  loupistId: string;
  loupistName: string;
}) {
  useTranslation(["assets", "chat"]);
  const record = message.earnings!;
  const go = useGo();
  const name = useAssetNames(bridge, loupistId)({ name: record.name, assetId: record.asset_id, symbol: record.symbol, market: record.market });
  const [retrying, setRetrying] = useState(false);
  const [state, setState] = useState<"idle" | "captured" | "failed">("idle");
  const at = Date.parse(message.created_at);
  const page = { loupistId, assetId: record.asset_id, name, section: "results" as const, eventId: record.event_id };
  const open = () => go?.({ kind: "asset", target: page });
  const openEvidence = (_source: AssetEvidence, citation: ChatEvidenceRef) => go?.(hostSourceDestination(citation, page));
  const retry = async () => {
    if (!bridge || retrying) return;
    setRetrying(true);
    try { await bridge.earningsPrepare({ loupistId, assetId: record.asset_id, eventId: record.event_id }); setState("captured"); }
    catch { setState("failed"); }
    finally { setRetrying(false); }
  };
  return <article className="message assistant thread-earnings" data-outcome={record.outcome} aria-label={t("earnings.title", { period: record.period })}>
    <div className="message-meta">
      <span>{loupistName}</span>
      {Number.isFinite(at) ? <time dateTime={new Date(at).toISOString()}>{formatTime(at)}</time> : null}
    </div>
    {record.outcome === "reported" ? <>
      <p>{i18n.t("chat:thread.earningsReported", { name, period: record.period })}</p>
      {record.measures.length ? <EarningsMiniTable measures={record.measures} onOpenEvidence={openEvidence} /> : null}
      {go ? <div className="thread-earnings-actions"><Button type="button" variant="outline" size="sm" aria-label={`${t("earnings.seeBeforeAfter")} · ${name}`} onClick={open}>{t("earnings.seeBeforeAfter")}</Button></div> : null}
    </> : <>
      <p>{state === "captured" ? t("earnings.captured") : t("earnings.failed", { name })}</p>
      <div className="thread-earnings-actions">
        {state !== "captured" && bridge ? <Button type="button" variant="outline" size="sm" disabled={retrying} aria-label={`${t("earnings.retry")} · ${name}`} onClick={() => void retry()}>{retrying ? <Spinner /> : null}{t("earnings.retry")}</Button> : null}
        {state === "captured" && go ? <Button type="button" variant="outline" size="sm" onClick={open}>{t("earnings.seeBeforeAfter")}</Button> : null}
      </div>
      {state === "failed" ? <p className="inline-error" role="alert">{t("earnings.failed", { name })}</p> : null}
    </>}
  </article>;
}
