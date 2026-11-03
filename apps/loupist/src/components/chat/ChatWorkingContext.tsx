import { formatDateTime } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { knowledgeLocationFromReference } from "../../knowledge/location";
import { bodyWithoutRepeatedTitle } from "../../knowledge/reading";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { FileTextIcon, MessageSquareIcon } from "lucide-react";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../ui/dialog";
import { SafeMessageResponse } from "./SafeMessageResponse";
import { errorMessage } from "../../lib/errors";
import { isEmptyChatThreadId } from "../../app/emptyChat";
import type { LoupistBridge, WorkingNote, KnowledgeLocation, KnowledgeReference } from "../../types";

const t = i18n.getFixedT(null, "chat");

type Props = {
  bridge: LoupistBridge;
  loupistId: string;
  chatId: string;
  isRunning: boolean;
  updateKey: string;
  onAskForChange: () => void;
  onOpenLink: (url: string) => Promise<void>;
  onOpenKnowledge?: (location: KnowledgeLocation, title: string) => void;
  children: ReactNode;
};

const savedAt = (value: number) => formatDateTime(value, { dateStyle: "medium", timeStyle: "short" });

/** Mounted per conversation, so a delayed response cannot replace another chat's note. */
export function ChatWorkingContext({ bridge, loupistId, chatId, isRunning, updateKey,
  onAskForChange, onOpenLink, onOpenKnowledge, children }: Props) {
  useTranslation("chat");
  const [note, setNote] = useState<WorkingNote | null>(null);
  const [readerOpen, setReaderOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const generation = useRef(0);

  useEffect(() => {
    const ticket = ++generation.current;
    if (isRunning) return;
    if (!chatId || isEmptyChatThreadId(chatId)) return;
    void bridge.knowledgeWorkingNotes({ loupistId, chatId })
      .then((page) => {
        if (ticket !== generation.current) return;
        setNote(page.current);
        setError(null);
      }).catch((cause) => {
        if (ticket === generation.current) setError(errorMessage(cause, t("chatWorkingContext.couldNotLoadTheWorkingNote")));
      });
    return () => { generation.current += 1; };
  }, [bridge, loupistId, chatId, isRunning, updateKey, refresh]);

  const evidence: { reference: KnowledgeReference; label: string }[] = [
    ...(note?.inputs?.sources ?? []).map((source, index) => ({
      label: source.attribution ? t("chatWorkingContext.attributedSource", { attribution: source.attribution }) : t("chatWorkingContext.numberedSource", { value1: index + 1 }),
      reference: { kind: "source" as const, recordId: source.recordId, revisionId: source.revisionId, objectId: source.spanId, spanId: source.spanId, receiptId: source.receiptId },
    })),
    ...(note?.inputs?.worlds ?? []).map((world, index) => ({
      label: t("chatWorkingContext.numberedWorld", { value1: index + 1 }),
      reference: { kind: "world" as const, recordId: world.recordId, revisionId: world.revisionId, objectId: world.recordId },
    })),
    ...(note?.inputs?.lenses ?? []).map((lens, index) => ({
      label: t("chatWorkingContext.numberedLens", { value1: index + 1, value2: lensUseLabel(lens.status) }),
      reference: { kind: "lens" as const, recordId: lens.recordId, revisionId: lens.revisionId, objectId: lens.recordId },
    })),
  ];

  return (
    <div className="chat-working-context">
      {note ? <div className="working-context-tabs">
        <Button size="sm" variant="ghost" onClick={() => setReaderOpen(true)}><FileTextIcon />{t("chatWorkingContext.workingNote")}</Button>
      </div> : null}
      {error ? <div className="working-context-error" role="alert">{error}<Button variant="ghost" size="sm" onClick={() => setRefresh((value) => value + 1)}>{t("chatWorkingContext.tryAgain")}</Button></div> : null}
      <div className="working-context-conversation">{children}</div>
      <Dialog open={readerOpen} onOpenChange={setReaderOpen}>
        <DialogContent className="working-note-dialog sm:max-w-2xl">
          <DialogHeader><DialogTitle>{note?.title ?? t("chatWorkingContext.workingNote")}</DialogTitle>
          <DialogDescription>{t("chatWorkingContext.theCurrentResearchNoteForThisConversation")}</DialogDescription></DialogHeader>
          {note ? <article className="working-note" aria-label={t("chatWorkingContext.workingNote")}>
            <span className="working-note-date">{t("chatWorkingContext.updatedDate", { date: savedAt(note.recordedAtMs) })}</span>
            {note.changeSummary.trim() ? <p className="note-change-summary">{note.changeSummary}</p> : null}
            <SafeMessageResponse key={note.researchId} text={bodyWithoutRepeatedTitle(note.title, note.markdown)} isAnimating={false} onOpenLink={onOpenLink} />
            {evidence.length ? <section aria-label={t("chatWorkingContext.noteEvidence")}><h3>{t("chatWorkingContext.evidenceUsed")}</h3><ul>{evidence.map(({ reference, label }) => <li key={`${reference.kind}:${reference.recordId}:${reference.revisionId}:${reference.objectId}`}>
              <Button variant="link" disabled={!onOpenKnowledge} onClick={() => { setReaderOpen(false); onOpenKnowledge?.(knowledgeLocationFromReference(loupistId, reference), label); }}>{label}</Button>
            </li>)}</ul></section> : null}
          </article> : null}
          {error ? <p className="working-context-error" role="alert">{error}</p> : null}
          {note ? <div className="working-note-actions">
              <Button variant="ghost" size="sm" onClick={() => { setReaderOpen(false); onAskForChange(); }}><MessageSquareIcon />{t("chatWorkingContext.askForAChange")}</Button>
            </div> : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}

const lensUseLabel = (status: string) =>
  ({
    considered: t("chatWorkingContext.considered"),
    applied: t("chatWorkingContext.applied"),
    adapted: t("chatWorkingContext.adapted"),
    rejected: t("chatWorkingContext.rejected"),
    available: t("chatWorkingContext.available"),
    archived: t("chatWorkingContext.archived"),
    unresolved: t("chatWorkingContext.unresolved"),
  })[status] ?? t("chatWorkingContext.saved");
