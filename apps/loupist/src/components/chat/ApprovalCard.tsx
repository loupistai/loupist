import i18n, { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
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

type NativeKind = "command" | "file_read" | "file_write" | "instructions";
const isNative = (kind: ChatApproval["kind"]): kind is NativeKind =>
  kind === "command" || kind === "file_read" || kind === "file_write" || kind === "instructions";

/** The card's heading, body and note for one kind; `target` is the site, path or command line. */
function cardCopy(approval: ChatApproval, tool: string) {
  const target = approval.target ?? "";
  switch (approval.kind) {
    case "web_site":
      return { label: t("approvalCard.webLabel", { site: target }), body: t("approvalCard.webBody", { site: target }), note: t("approvalCard.webNote") };
    case "command":
      return {
        label: t("approvalCard.commandLabel"),
        body: t("approvalCard.commandBody"),
        note: t("approvalCard.commandNote"),
      };
    case "file_read":
      return { label: t("approvalCard.readLabel", { path: target }), body: t("approvalCard.readBody", { path: target }), note: t("approvalCard.readNote") };
    case "file_write":
      return { label: t("approvalCard.writeLabel", { path: target }), body: t("approvalCard.writeBody", { path: target }), note: t("approvalCard.writeNote") };
    case "instructions":
      return { label: t("approvalCard.instructionsLabel", { path: target }), body: t("approvalCard.instructionsBody", { path: target }), note: t("approvalCard.instructionsNote") };
    default:
      return {
        label: t("approvalCard.label", { tool }),
        body: approval.target
          ? t("approvalCard.body", { connection: approval.connectionName, tool, target: approval.target })
          : t("approvalCard.bodyNoTarget", { connection: approval.connectionName, tool }),
        note: t("approvalCard.readOnly"),
      };
  }
}

/** The whole command, wrapped, with its line count so a block that scrolls is never mistaken for the whole. */
function CommandBlock({ command }: { command: string }) {
  const lines = command.split("\n").length;
  return <>
    <pre className="approval-card-command" tabIndex={0}>{command}</pre>
    <p className="approval-card-command-lines">{t("approvalCard.commandLines", { count: lines, formattedCount: formatNumber(lines) })}</p>
  </>;
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
  // A blanket Allow exists for a custom connection (stored in Settings) and for a
  // command (this reply).
  const always = approval.kind === undefined ? t("approvalCard.always")
    : approval.kind === "command" ? t("approvalCard.commandAlwaysReply")
      : null;
  const allow = approval.kind === "file_read" || approval.kind === "file_write" ? t("approvalCard.allow") : t("approvalCard.allowOnce");
  // The rail's "Needs your OK" row scrolls here and moves focus to the card, never to a button.
  return <section className="approval-card" data-kind={approval.kind} data-request-id={approval.requestId} tabIndex={-1} aria-label={copy.label}>
    <header>
      <strong>{t("approvalCard.title")}</strong>
      <span className="approval-card-clock">{t("approvalCard.waiting", { time: clock(nowMs - approval.requestedAtMs) })}</span>
    </header>
    <p>{copy.body}</p>
    {approval.kind === "command" ? <CommandBlock command={approval.detail ?? approval.target ?? ""} /> : null}
    <p className="approval-card-note">{copy.note}</p>
    <div className="approval-card-actions">
      <Button type="button" disabled={busy} onClick={() => void decide("once")}>{allow}</Button>
      <Button type="button" variant="outline" disabled={busy} onClick={() => void decide("deny")}>{t("approvalCard.notNow")}</Button>
      {always !== null ? <Button type="button" variant={approval.kind === "command" ? "secondary" : "ghost"} className="approval-card-always" disabled={busy} onClick={() => void decide("always")}>{always}</Button> : null}
    </div>
    {failed ? <p role="alert" className="approval-card-error">{t("approvalCard.error")}</p> : null}
  </section>;
}

/** One quiet line for a settled native card. */
function NativeLine({ approval, kind }: { approval: ChatApproval; kind: NativeKind }) {
  const target = approval.target ?? "";
  const key = kind === "command" ? "command" : kind === "file_read" ? "read" : kind === "file_write" ? "write" : "instructions";
  const text = approval.state === "declined" ? t(`approvalCard.${key}Declined`, { target })
    : approval.state === "expired" ? t(`approvalCard.${key}Expired`, { target })
      : kind === "command" && approval.state === "always_allowed" ? t("approvalCard.commandAllowedReply")
        : t(`approvalCard.${key}Allowed`, { target });
  return <p className="approval-line" data-state={approval.state} data-kind={kind}>{text}</p>;
}

/**
 * Approval cards for a reply (composer spec D3): a waiting card with Allow once,
 * Always allow for this connection and Not now, and one quiet line once decided.
 * The answer is the only place with the buttons; the rail shows one "Needs your
 * OK" row that leads here. "Always" is stored by Rust per connection and tool and
 * is revocable in the connection's settings. Refusals from runs that could not
 * ask (scheduled runs) point to the same settings. A new website after
 * third-party text offers only Allow once and Not now. A native card (a command,
 * a file outside the workspace, an instruction file) shows what would run or be
 * touched; a command's blanket Allow covers this reply.
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
      if (isNative(approval.kind)) return <NativeLine key={approval.requestId} approval={approval} kind={approval.kind} />;
      if (approval.kind === "web_site") {
        const site = approval.target ?? "";
        return <p key={approval.requestId} className="approval-line" data-state={approval.state}>
          {approval.state === "allowed_once" || approval.state === "always_allowed" ? t("approvalCard.webAllowedOnce", { site })
            : approval.state === "declined" ? t("approvalCard.webDeclined", { site })
              : t("approvalCard.webExpired", { site })}
        </p>;
      }
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
