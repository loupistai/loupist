import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState } from "react";
import type { LoupistBridge } from "../../types";
import type { ChatApproval, ChatApprovalDecision, ChatToolRefusal } from "../../contracts/chat";
import type { CustomConnection } from "../../contracts/customConnections";
import { Button } from "../ui/button";
import { ConnectionDetail } from "../connections/custom/ConnectionDetail";
import { humanizeToolName } from "../connections/custom/toolTitle";

const t = i18n.getFixedT(null, "chat");

const clock = (ms: number) => {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
};

/** Opens a connection's detail sheet, where approvals are reviewed and revoked. */
function useConnectionDetail(bridge: LoupistBridge, loupistId: string) {
  const [selected, setSelected] = useState<CustomConnection | null>(null);
  const [failed, setFailed] = useState(false);
  const open = async (connectionId: string) => {
    setFailed(false);
    try {
      const connection = (await bridge.customConnectionList({ loupist_id: loupistId })).find((record) => record.id === connectionId);
      if (!connection) throw new Error("Connection unavailable");
      setSelected(connection);
    } catch {
      setFailed(true);
    }
  };
  const sheet = selected ? <ConnectionDetail bridge={bridge} connection={selected} loupistId={loupistId}
    onChanged={() => void open(selected.id)} onClose={() => setSelected(null)} onRemoved={() => setSelected(null)} /> : null;
  return { open, sheet, failed };
}

/** The card's heading, body and note; `target` is the call's plain values. */
function cardCopy(approval: ChatApproval, tool: string) {
  return {
    label: t("approvalCard.label", { tool }),
    body: approval.target
      ? t("approvalCard.body", { connection: approval.connectionName, tool, target: approval.target })
      : t("approvalCard.bodyNoTarget", { connection: approval.connectionName, tool }),
    note: t("approvalCard.readOnly"),
  };
}

function WaitingCard({ approval, onDecide }: { approval: ChatApproval; onDecide: (decision: ChatApprovalDecision) => Promise<void> }) {
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    const timer = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const tool = humanizeToolName(approval.toolName);
  const copy = cardCopy(approval, tool);
  const decide = async (decision: ChatApprovalDecision) => {
    setBusy(true);
    setFailed(false);
    try {
      await onDecide(decision);
    } catch {
      setFailed(true);
      setBusy(false);
    }
  };
  // The rail's "Needs your OK" row scrolls here and moves focus to the card, never to a button.
  return <section className="approval-card" data-request-id={approval.requestId} tabIndex={-1} aria-label={copy.label}>
    <header>
      <strong>{t("approvalCard.title")}</strong>
      <span className="approval-card-clock">{t("approvalCard.waiting", { time: clock(nowMs - approval.requestedAtMs) })}</span>
    </header>
    <p>{copy.body}</p>
    <p className="approval-card-note">{copy.note}</p>
    <div className="approval-card-actions">
      <Button type="button" disabled={busy} onClick={() => void decide("once")}>{t("approvalCard.allowOnce")}</Button>
      <Button type="button" variant="outline" disabled={busy} onClick={() => void decide("deny")}>{t("approvalCard.notNow")}</Button>
      <Button type="button" variant="ghost" className="approval-card-always" disabled={busy} onClick={() => void decide("always")}>{t("approvalCard.always")}</Button>
    </div>
    {failed ? <p role="alert" className="approval-card-error">{t("approvalCard.error")}</p> : null}
  </section>;
}

/**
 * Approval cards for a reply (composer spec D3): a waiting card with Allow once,
 * Always allow for this connection and Not now, and one quiet line once decided.
 * The answer is the only place with the buttons; the rail shows one "Needs your
 * OK" row that leads here. "Always" is stored by Rust per connection and tool and
 * is revocable in the connection's settings. Refusals from runs that could not
 * ask (scheduled runs) point to the same settings.
 */
export function ApprovalCards({ approvals, refusals, bridge, loupistId }: {
  approvals: readonly ChatApproval[];
  refusals: readonly ChatToolRefusal[];
  bridge: LoupistBridge;
  loupistId: string;
}) {
  useTranslation("chat");
  const detail = useConnectionDetail(bridge, loupistId);
  const settingsOnly = refusals.filter((refusal) => refusal.reason === "needs_approval" || refusal.reason === "blocked_unattended");
  if (!approvals.length && !settingsOnly.length) return null;
  const decide = (approval: ChatApproval) => (decision: ChatApprovalDecision) =>
    bridge.chatApprovalDecide({ loupist_id: loupistId, request_id: approval.requestId, decision });
  return <div className="approval-cards">
    {approvals.map((approval) => {
      const tool = humanizeToolName(approval.toolName);
      if (approval.state === "waiting") return <WaitingCard key={approval.requestId} approval={approval} onDecide={decide(approval)} />;
      return <p key={approval.requestId} className="approval-line" data-state={approval.state}>
        {approval.state === "allowed_once" ? t("approvalCard.allowedOnce", { tool })
          : approval.state === "always_allowed" ? <>{t("approvalCard.alwaysAllowed", { tool, connection: approval.connectionName })} · <Button type="button" variant="link" onClick={() => void detail.open(approval.connectionId)}>{t("approvalCard.changeInSettings")}</Button></>
            : approval.state === "declined" ? t("approvalCard.declined", { tool })
              : t("approvalCard.expired", { tool })}
      </p>;
    })}
    {settingsOnly.map((refusal, index) => <p key={`${refusal.connection_id}:${refusal.tool_name}:${index}`} className="approval-line" data-state="settings" tabIndex={-1}>
      <span title={refusal.tool_name}>{t("approvalCard.needsSettings", { connection: refusal.connection_name, tool: humanizeToolName(refusal.tool_name) })}</span>
      {" "}<Button type="button" variant="link" onClick={() => void detail.open(refusal.connection_id)}>{t("approvalCard.review")}</Button>
    </p>)}
    {detail.failed ? <p role="alert" className="approval-card-error">{i18n.t("settings:custom.error")}</p> : null}
    {detail.sheet}
  </div>;
}
