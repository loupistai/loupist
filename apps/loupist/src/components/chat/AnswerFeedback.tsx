import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useId, useRef, useState } from "react";
import { ThumbsDownIcon, ThumbsUpIcon } from "lucide-react";
import { SavedPrinciple } from "./SavedPrinciple";
import { PrincipleForm } from "../map/PrincipleForm";
import type { LoupistBridge, Principle, ChatFeedback, ChatMessage } from "../../types";
import { useGo } from "../navigation/Open";
import { feedbackTeachingPrompt } from "../../chat/answerFeedback";
import { errorMessage } from "../../lib/errors";

const t = i18n.getFixedT(null, "chat");

export type SaveAnswerFeedback = (messageId: string, value: Pick<ChatFeedback, "rating" | "correction">) => Promise<ChatFeedback>;

const busyReason = () => t("answerFeedback.availableWhenTheReplyFinishes");

export function AnswerFeedback({ message, onSave, onTeach, feedbackBusy = false, bridge, loupistId, chatId }: {
  bridge?: LoupistBridge; loupistId?: string; chatId?: string;
  message: ChatMessage;
  onSave: SaveAnswerFeedback;
  onTeach: (text: string) => void;
  feedbackBusy?: boolean;
}) {
  useTranslation("chat");
  const go = useGo();
  const [feedback, setFeedback] = useState(message.feedback);
  const [principleForm, setPrincipleForm] = useState(false);
  const [savedPrinciple, setSavedPrinciple] = useState<Principle>();
  const [archiving, setArchiving] = useState(false);
  const [principleError, setPrincipleError] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(message.feedback?.correction ?? "");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const editorId = useId();
  const busyReasonId = useId();
  const mounted = useRef(true);
  const busy = useRef(false);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => { setFeedback(message.feedback); }, [message.feedback]);
  const unavailable = feedbackBusy || pending;
  const save = async (rating: ChatFeedback["rating"], correction: string | null, close = false) => {
    if (busy.current || feedbackBusy) return;
    busy.current = true;
    setPending(true);
    setError("");
    try {
      const saved = await onSave(message.id, { rating, correction });
      if (!mounted.current) return;
      if (saved.correction !== feedback?.correction) { setSavedPrinciple(undefined); setPrincipleForm(false); }
      setFeedback(saved);
      if (close) setEditing(false);
    } catch (cause) {
      if (mounted.current) setError(errorMessage(cause, t("answerFeedback.couldNotSaveFeedback")));
    } finally {
      busy.current = false;
      if (mounted.current) setPending(false);
    }
  };
  const undoPrinciple = async () => {
    if (!bridge || !loupistId || !savedPrinciple || archiving) return;
    setArchiving(true); setPrincipleError(false);
    try { await bridge.principleArchive({ loupistId, recordId: savedPrinciple.recordId, archived: true }); setSavedPrinciple(undefined); }
    catch { setPrincipleError(true); }
    finally { setArchiving(false); }
  };
  const viewPrinciple = () => {
    if (!bridge || !loupistId || !savedPrinciple) return;
    // A principle has no page for the Evidence reader to show; it opens on "What {Loupist} knows".
    go?.({ kind: "page", location: { loupistId, surface: "lens", recordId: savedPrinciple.recordId } });
  };
  const showCorrection = editing || Boolean(feedback?.correction) || Boolean(error);
  return <>
    <div className="answer-feedback">
      <div className="message-actions" aria-describedby={feedbackBusy ? busyReasonId : undefined}>
        {feedbackBusy ? <span id={busyReasonId} className="sr-only">{busyReason()}</span> : null}
        <button type="button" aria-label={t("answerFeedback.helpful")} title={feedbackBusy ? busyReason() : t("answerFeedback.helpful")} aria-pressed={feedback?.rating === "up"} aria-describedby={feedbackBusy ? busyReasonId : undefined} disabled={unavailable} onClick={() => void save(feedback?.rating === "up" ? null : "up", feedback?.correction ?? null)}><ThumbsUpIcon /></button>
        <button type="button" aria-label={t("answerFeedback.notHelpful")} title={feedbackBusy ? busyReason() : t("answerFeedback.notHelpful")} aria-pressed={feedback?.rating === "down"} aria-describedby={feedbackBusy ? busyReasonId : undefined} disabled={unavailable} onClick={() => void save(feedback?.rating === "down" ? null : "down", feedback?.correction ?? null)}><ThumbsDownIcon /></button>
        <button type="button" aria-expanded={editing} aria-describedby={feedbackBusy ? busyReasonId : undefined} title={feedbackBusy ? busyReason() : t("answerFeedback.correct")} {...(editing ? { "aria-controls": editorId } : {})} disabled={unavailable} onClick={() => { if (!editing) setDraft(feedback?.correction ?? ""); setEditing(true); }}>{t("answerFeedback.correct")}</button>
      </div>
    </div>
    {showCorrection ? <div className="answer-correction" id={editing ? editorId : undefined}>
      {editing ? <>
        <label htmlFor={`${editorId}-text`}>{t("answerFeedback.yourCorrection")}</label>
        <textarea id={`${editorId}-text`} value={draft} onChange={(event) => setDraft(event.target.value)} rows={3} disabled={unavailable} autoFocus />
        <small>{t("answerFeedback.characterCount", { count: [...draft.trim()].length, formattedCount: formatNumber([...draft.trim()].length) })}</small>
        <div className="message-actions">
          <button type="button" aria-describedby={feedbackBusy ? busyReasonId : undefined} disabled={unavailable || [...draft.trim()].length > 4000} onClick={() => void save(feedback?.rating ?? null, draft, true)}>{pending ? t("answerFeedback.saving") : t("answerFeedback.saveCorrection")}</button>
          <button type="button" aria-describedby={feedbackBusy ? busyReasonId : undefined} disabled={unavailable} onClick={() => setEditing(false)}>{t("answerFeedback.cancel")}</button>
        </div>
      </> : null}
      {feedback?.correction && !editing ? <>
        {savedPrinciple && savedPrinciple.statement.trim() === feedback.correction.trim() ? null : <>
          <small className="answer-saved-correction-label">{t("answerFeedback.savedCorrection")}</small>
          <p className="answer-saved-correction">{feedback.correction}</p>
        </>}
        {savedPrinciple ? <SavedPrinciple className="principle-saved" statement={savedPrinciple.statement} pending={archiving} onUndo={() => void undoPrinciple()} onView={viewPrinciple} /> : <div className="message-actions">
          <button type="button" aria-describedby={feedbackBusy ? busyReasonId : undefined} disabled={unavailable} onClick={() => onTeach(feedbackTeachingPrompt(message.content, feedback.correction!))}>{t("answerFeedback.teachTheLoupist")}</button>
          {bridge && loupistId && chatId ? <button type="button" data-feedback-principle disabled={unavailable} onClick={() => setPrincipleForm(true)}>{t("principles.save")}</button> : null}
        </div>}
      </> : null}
      {principleForm && bridge && loupistId && chatId ? <PrincipleForm bridge={bridge} loupistId={loupistId} initialStatement={[...(feedback?.correction ?? "")].slice(0, 280).join("")} origin="feedback" originChatId={chatId} originMessageId={message.id} onCancel={() => setPrincipleForm(false)} onSaved={saved => { setSavedPrinciple(saved); setPrincipleForm(false); }} /> : null}
      {principleError ? <p role="alert">{t("principles.error")}</p> : null}
      {error ? <p role="alert">{error}</p> : null}
    </div> : null}
  </>;
}
