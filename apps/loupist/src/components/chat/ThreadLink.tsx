import i18n, { useTranslation } from "@/i18n";
import { ChevronRightIcon } from "lucide-react";
import { useGo } from "../navigation/Open";
import { useEffect, useRef, useState } from "react";
import type { ChatMessage, ChatThread, HomeHandoff, LoupistBridge, ResearchHandoffChoices } from "../../types";
import { commandErrorCode } from "../../lib/errors";
import { relationName } from "../map/relations";
import { BotPostBody } from "./BotPost";
import "@/styles/bot-cards.css";

type LinkError = "startError" | "error" | "conflict";
const CONFLICT_ERROR: LinkError = "conflict";

const t = i18n.getFixedT(null, "chat");

type Outcome = "dismissed" | "gone";

export function ThreadLink({ message, bridge, loupistId, loupistName, composerChoices, research }: {
  message: ChatMessage;
  bridge?: LoupistBridge;
  loupistId: string;
  loupistName: string;
  /** The Bot composer's model, level, run options and Memory choices at the moment of the click. */
  composerChoices?: () => ResearchHandoffChoices;
  research?: HomeHandoff;
}) {
  useTranslation("chat");
  const go = useGo();
  const { t: tk } = useTranslation("knowledge");
  const link = message.link!;
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [start, setStart] = useState<{ id: string; previousId?: string } | null>(null);
  const [error, setError] = useState<LinkError | null>(null);
  const [busy, setBusy] = useState(false);
  // Clicks land before React redraws the disabled buttons: one request at a time.
  const pending = useRef(false);
  const statusRef = useRef<HTMLParagraphElement>(null);
  // The buttons that held focus go away with the outcome: focus moves to what happened.
  useEffect(() => { if (outcome || start) statusRef.current?.focus({ preventScroll: true }); }, [outcome, start]);
  // A receipt confirms the start before Bot's delivery arrives. The persisted
  // delivery supplies the current state once this request appears in the thread.
  const awaitingDelivery = start && research?.handoffId === start.previousId && research?.handoffId !== start.id;
  const status = awaitingDelivery ? "running" : research?.status;
  const notice = outcome ?? (status === "running" || status === "created" ? "opened" : status);
  const canAct = !outcome && status !== "running" && status !== "created";
  const at = Date.parse(message.created_at);
  const through = link.path.slice(0, -1).map((hop) => hop.name).join(", ");
  const names = { seed: link.seedName, far: link.farName, through };
  const act = async (action: () => Promise<void>, failed: "startError" | "error") => {
    if (!bridge || pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (cause) {
      const code = commandErrorCode(cause);
      if (code === "not_found") setOutcome("gone");
      else if (code === "conflict" && failed === "startError") setError(CONFLICT_ERROR);
      else setError(failed);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  // The host writes the request from its own link row and marks the link opened in the same step.
  const lookInto = () => act(async () => {
    const receipt = await bridge!.researchHandoffStart({ loupistId, connectionId: link.connectionId, ...composerChoices?.() });
    setStart({ id: receipt.handoffId, previousId: research?.handoffId });
  }, "startError");
  const notUseful = () => act(async () => {
    await bridge!.knowledgeConnectionDismiss({ loupistId, connectionId: link.connectionId });
    setOutcome("dismissed");
  }, "error");
  // The path stays one hover away; the post itself is one quiet line, never a call to action.
  const pathText = [link.seedName, ...link.path.flatMap((hop) => [hop.reversed ? "←" : "→", relationName(hop.relation, tk), hop.reversed ? "←" : "→", hop.name])].join(" ");
  return <article className="message assistant thread-alert thread-link" data-bot-post="" aria-label={t("link.label", names)}>
    <BotPostBody loupistName={loupistName} at={at}>
      <p className="bot-quiet-line">
        <span>{t("link.message", names)}</span>
        {/* The visible words are its name, so voice control finds it; the path is its tooltip. */}
        {go ? <button type="button" className="bot-quiet-action" title={pathText}
          onClick={() => go({ kind: "page", location: { loupistId, surface: "world", view: { highlightEntityIds: link.nodeIds } } })}>{t("link.showPath")}</button> : null}
        {/* Waiting keeps the buttons focusable, so a failed start leaves the keyboard where it was. */}
        {bridge && canAct ? <>
          <button type="button" className="bot-quiet-action" aria-disabled={busy || undefined} aria-busy={busy || undefined} onClick={() => void lookInto()}>{t(status ? "link.lookAgain" : "link.lookInto")}</button>
          <button type="button" className="bot-quiet-action" aria-disabled={busy || undefined} onClick={() => void notUseful()}>{t("link.notUseful")}</button>
        </> : null}
      </p>
      {notice ? <p ref={statusRef} tabIndex={-1} className="thread-delivery-source" role={notice === "gone" ? "alert" : "status"}>{t(`link.${notice}`)}</p> : null}
      {error && !(error === "conflict" && research) ? <p className="inline-error" role="alert">{t(`link.${error}`)}</p> : null}
    </BotPostBody>
  </article>;
}

/** The Bot reply opens its research session both while the host runs it and after it finishes. */
export function ResearchSessionLink({ session }: { session: ChatThread }) {
  const { t } = useTranslation("chat");
  const go = useGo();
  if (!go) return null;
  return <div className="message-schedules">
    <button type="button" className="message-schedule-open" onClick={() => go({ kind: "chat", threadId: session.id })}>
      <span className="message-schedule-summary">
        <span className="message-schedule-heading"><strong title={session.title}>{session.title}</strong></span>
        {session.handoff_run?.run_id && <span className="message-schedule-cadence">{t("link.opened")}</span>}
      </span>
      <ChevronRightIcon aria-hidden="true" />
    </button>
  </div>;
}
