import i18n, { useTranslation } from "@/i18n";
import { formatTime } from "@/i18n/format";
import { useState } from "react";
import type { ChatMessage, LoupistBridge } from "../../types";
import { relationName } from "../map/relations";
import { Button } from "../ui/button";

const t = i18n.getFixedT(null, "chat");

/**
 * "New link: A ↔ C through B", posted once into the home Thread when a World
 * publication joined something the user follows to a name it could not reach
 * before. The path is the whole claim: no alpha, only "new to this Loupist".
 * Look into it hands the question to a research session the same way the Bot
 * does; Not useful keeps the link from coming back.
 */
export function ThreadLink({ message, bridge, loupistId, loupistName, onOpenMap }: {
  message: ChatMessage;
  bridge?: LoupistBridge;
  loupistId: string;
  loupistName: string;
  /** Opens the Map with these names highlighted, seed first. */
  onOpenMap?: (nodeIds: string[]) => void;
}) {
  useTranslation("chat");
  const { t: tk } = useTranslation("knowledge");
  const link = message.link!;
  const [outcome, setOutcome] = useState<"opened" | "dismissed" | null>(null);
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);
  const at = Date.parse(message.created_at);
  const through = link.path.slice(0, -1).map((hop) => hop.name).join(", ");
  const names = { seed: link.seedName, far: link.farName, through };
  const act = async (action: () => Promise<unknown>, done: "opened" | "dismissed") => {
    if (!bridge || busy) return;
    setBusy(true);
    setError(false);
    try {
      await action();
      setOutcome(done);
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  };
  const lookInto = () => act(async () => {
    await bridge!.researchHandoffStart({
      loupistId,
      title: t("link.title", names),
      question: t("link.question", names),
      pins: link.nodeIds.map((nodeId) => ({ world: nodeId })),
      requestKey: `link-${link.connectionId}`,
    });
    await bridge!.knowledgeConnectionOpen({ loupistId, connectionId: link.connectionId });
  }, "opened");
  const notUseful = () => act(() => bridge!.knowledgeConnectionDismiss({ loupistId, connectionId: link.connectionId }), "dismissed");
  const path = <>
    <span className="thread-link-name">{link.seedName}</span>
    {link.path.map((hop, index) => <span key={`${hop.name}-${index}`} className="thread-link-hop">
      <span className="thread-link-relation" aria-hidden="true">{hop.reversed ? "←" : "→"}</span>
      <span className="thread-link-relation">{relationName(hop.relation, tk)}</span>
      <span className="thread-link-relation" aria-hidden="true">{hop.reversed ? "←" : "→"}</span>
      <span className="thread-link-name">{hop.name}</span>
    </span>)}
  </>;
  return <article className="message assistant thread-alert" aria-label={t("link.label", names)}>
    <div className="message-meta">
      <span>{loupistName}</span>
      {Number.isFinite(at) ? <time dateTime={new Date(at).toISOString()}>{formatTime(at)}</time> : null}
    </div>
    <p>{t("link.message", names)}</p>
    {onOpenMap
      ? <button type="button" className="thread-link-path morning-link" aria-label={t("link.pathLabel")} onClick={() => onOpenMap(link.nodeIds)}>{path}</button>
      : <p className="thread-link-path">{path}</p>}
    {outcome ? <p className="thread-delivery-source" role="status">{t(`link.${outcome}`)}</p> : null}
    {error ? <p className="inline-error" role="alert">{t("link.error")}</p> : null}
    {bridge && !outcome ? <div className="thread-alert-actions">
      <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void lookInto()}>{t("link.lookInto")}</Button>
      <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void notUseful()}>{t("link.notUseful")}</Button>
    </div> : null}
  </article>;
}
