import i18n, { useTranslation } from "@/i18n";
import { useLayoutEffect, useRef, useState } from "react";
import type { ChatMessage, LoupistBridge } from "../../types";
import { BotPostBody } from "./BotPost";
import "@/styles/bot-cards.css";

const t = i18n.getFixedT(null, "chat");

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
  // Undo goes away once it has run: focus moves to what happened.
  const statusRef = useRef<HTMLParagraphElement>(null);
  useLayoutEffect(() => {
    if (!undone) return;
    const focused = document.activeElement;
    if (!focused || focused === document.body) statusRef.current?.focus({ preventScroll: true });
  }, [undone]);
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
  return <article className="message assistant thread-alert thread-vocabulary" data-bot-post="" aria-label={t("vocabulary.label", { list })}>
    <BotPostBody loupistName={loupistName} at={at}>
      <p className="bot-quiet-line">
        <span>{t("vocabulary.message", { list })}</span>
        {/* Waiting keeps the button focusable, so a failed Undo leaves the keyboard where it was. */}
        {bridge && !undone ? <button type="button" className="bot-quiet-action" aria-disabled={busy || undefined} onClick={() => void undo()}>{t("vocabulary.undo")}</button> : null}
      </p>
      {undone ? <p ref={statusRef} tabIndex={-1} className="thread-delivery-source" role="status">{t("vocabulary.undone")}</p> : null}
      {error ? <p className="inline-error" role="alert">{t("vocabulary.error")}</p> : null}
    </BotPostBody>
  </article>;
}
