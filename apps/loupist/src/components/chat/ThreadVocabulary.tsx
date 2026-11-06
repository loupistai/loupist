import i18n, { useTranslation } from "@/i18n";
import { formatTime } from "@/i18n/format";
import { useState } from "react";
import type { ChatMessage, LoupistBridge } from "../../types";
import { Button } from "../ui/button";

const t = i18n.getFixedT(null, "chat");

/**
 * "Your map now also tracks ...", posted once into the home Thread after a
 * starting setup or a background review changed what the map tracks. Undo
 * reverses only that change.
 */
export function ThreadVocabulary({ message, bridge, loupistId, loupistName }: {
  message: ChatMessage;
  bridge?: LoupistBridge;
  loupistId: string;
  loupistName: string;
}) {
  useTranslation("chat");
  const record = message.vocabulary!;
  const [undone, setUndone] = useState(false);
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);
  const korean = i18n.language.toLowerCase().startsWith("ko") && record.labelsKo?.length === record.labels.length;
  const list = (korean ? record.labelsKo! : record.labels).join(", ");
  const at = Date.parse(message.created_at);
  const undo = async () => {
    if (!bridge || busy) return;
    setBusy(true);
    setError(false);
    try {
      await bridge.worldSchemaUndo({ loupistId, changeId: record.changeId });
      setUndone(true);
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  };
  return <article className="message assistant thread-alert" aria-label={list}>
    <div className="message-meta">
      <span>{loupistName}</span>
      {Number.isFinite(at) ? <time dateTime={new Date(at).toISOString()}>{formatTime(at)}</time> : null}
    </div>
    <p>{t("vocabulary.message", { list })}</p>
    {undone ? <p className="thread-delivery-source" role="status">{t("vocabulary.undone")}</p> : null}
    {error ? <p className="inline-error" role="alert">{t("vocabulary.error")}</p> : null}
    {bridge && !undone ? <div className="thread-alert-actions">
      <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void undo()}>{t("vocabulary.undo")}</Button>
    </div> : null}
  </article>;
}
