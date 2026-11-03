import { Disclosure } from "../ui/disclosure";
import { ResearchJournalForm } from "./ResearchJournalForm";
import { JournalThenNow, ResearchJournalCheck } from "./ResearchJournal";
import { formatDateTime, formatNumber } from "../../i18n/format";
import { timePointValue, claimStatusLabel } from "../../knowledge/presentation";
import i18n, { useTranslation } from "../../i18n";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { knowledgeLocationFromReference } from "../../knowledge/location";
import { bodyWithoutRepeatedTitle } from "../../knowledge/reading";
import { errorMessage } from "../../lib/errors";
import type { JournalInputs, JournalRecord, JournalSourceRead, LoupistBridge, KnowledgeLocation, KnowledgePin, KnowledgeReference, WorkingNote } from "../../types";
import { SafeMessageResponse } from "../chat/SafeMessageResponse";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";

type Props = {
  bridge: LoupistBridge;
  location: KnowledgeLocation;
  onOpen: (location: KnowledgeLocation) => void;
  onAsk?: (pin: KnowledgePin, threadId?: string) => void;
  refreshKey?: number;
  onOpenLink?: (url: string) => void | Promise<void>;
  onReady?: () => void;
  onChanged?: () => void;
  onOpenChat?: (chatId: string) => void;
};

/** Saved notes and journals are exact readers opened from their Chat context. */
export function ResearchReader(props: Props) {
  return <ExactResearchReader key={JSON.stringify([props.location.loupistId, props.location.selection])} {...props} />;
}

function ExactResearchReader({ bridge, location, onOpen, onAsk, refreshKey = 0, onOpenLink, onReady, onChanged, onOpenChat }: Props) {
  const { t } = useTranslation("knowledge");
  const noteId = location.selection?.kind === "research" ? location.selection.recordId : undefined;
  const journalId = location.selection?.kind === "journal" ? location.selection.journalId : undefined;
  const [editing, setEditing] = useState(false);
  const [archiving, setArchiving] = useState(false);
  const [note, setNote] = useState<WorkingNote | null>(null);
  const [journal, setJournal] = useState<JournalRecord | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [reload, setReload] = useState(0);
  const [receipt, setReceipt] = useState<JournalSourceRead | null>(null);
  const [receiptError, setReceiptError] = useState<string | null>(null);
  const [requestedReceipt, setRequestedReceipt] = useState<string | null>(null);
  const [openingReceipt, setOpeningReceipt] = useState(false);
  const receiptGeneration = useRef(0);
  const readerPanel = useRef<HTMLElement>(null);
  const receiptPanel = useRef<HTMLElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  const restoreReceiptFocus = useRef(false);
  const readyCallback = useRef(onReady);
  readyCallback.current = onReady;

  useEffect(() => {
    let current = true;
    receiptGeneration.current += 1;
    setError(null); setReceipt(null); setReceiptError(null); setRequestedReceipt(null); setOpeningReceipt(false);
    if (!noteId && !journalId) { setReady(true); return; }
    setReady(false);
    const read = async () => {
      if (noteId) {
        const next = await bridge.knowledgeWorkingNoteRead({ loupistId: location.loupistId, noteId });
        if (!current) return;
        if (next.researchId !== noteId) throw new Error(t("researchReader.theRequestedWorkingNoteWasNotReturned"));
        setNote(next);
      } else {
        const next = await bridge.knowledgeJournalRead({ loupistId: location.loupistId, journalId: journalId! });
        if (!current) return;
        if (next.journalId !== journalId) throw new Error(t("researchReader.theRequestedJournalWasNotReturned"));
        setJournal(next);
      }
    };
    void read().catch((cause: unknown) => { if (current) setError(errorMessage(cause, t("researchReader.couldNotOpenTheSavedRecord"))); })
      .finally(() => { if (current) setReady(true); });
    return () => { current = false; receiptGeneration.current += 1; };
  }, [bridge, location.loupistId, noteId, journalId, refreshKey, reload]);

  useEffect(() => {
    if (note || journal) {
      readerPanel.current?.focus({ preventScroll: true });
      if (readyCallback.current) readyCallback.current();
      else readerPanel.current?.scrollIntoView({ block: "start" });
    }
  }, [note, journal]);
  useEffect(() => {
    if (receipt) {
      receiptPanel.current?.focus({ preventScroll: true });
      receiptPanel.current?.scrollIntoView({ block: "start" });
    } else if (restoreReceiptFocus.current) {
      restoreReceiptFocus.current = false;
      (returnFocus.current?.isConnected ? returnFocus.current : readerPanel.current)?.focus();
    }
  }, [receipt]);

  const openReference = (reference: KnowledgeReference) => onOpen(knowledgeLocationFromReference(location.loupistId, reference));
  const readReceipt = async (receiptId: string, trigger?: HTMLElement) => {
    const source = journal?.inputs.sources.find((entry) => entry.receiptId === receiptId);
    if (!journal || !source) return;
    const generation = ++receiptGeneration.current;
    if (trigger) returnFocus.current = trigger;
    setRequestedReceipt(receiptId); setOpeningReceipt(true); setReceipt(null); setReceiptError(null);
    try {
      const next = await bridge.knowledgeJournalSourceRead({ loupistId: location.loupistId, journalId: journal.journalId, receiptId });
      if (generation !== receiptGeneration.current) return;
      if (next.pinned.receiptId !== source.receiptId || next.pinned.recordId !== source.recordId || next.pinned.revisionId !== source.revisionId || next.pinned.spanId !== source.spanId) throw new Error(t("researchReader.theRequestedSavedSourceWasNotReturned"));
      setReceipt(next);
    } catch (cause) {
      if (generation === receiptGeneration.current) setReceiptError(errorMessage(cause, t("researchReader.couldNotReadTheSavedSource")));
    } finally { if (generation === receiptGeneration.current) setOpeningReceipt(false); }
  };
  const markdown = (text: string) => <SafeMessageResponse text={text} isAnimating={false} onOpenLink={onOpenLink ?? ((url) => bridge.openExternalUrl(url))} />;

  return <section className="knowledge-research-reader" aria-label={t("researchReader.savedResearchReader")}>
    {error && <div role="alert"><p>{error}</p><Button variant="outline" onClick={() => setReload((value) => value + 1)}>{t("researchReader.retrySavedRecord")}</Button></div>}
    {!ready && <p className="memory-loading-inline" role="status"><Spinner />{t("researchReader.loadingSavedRecord")}</p>}
    {ready && !noteId && !journalId && <p>{t("researchReader.openASavedWorkingNoteOrJournalFromChat")}</p>}
    {note && <article ref={readerPanel} tabIndex={-1} className="knowledge-reading knowledge-note-reader" aria-label={t("researchReader.savedWorkingNote")}>
      <header className="research-document-head"><div className="memory-meta-row"><Badge variant="outline">{t("researchReader.workingNote")}</Badge><time dateTime={new Date(note.recordedAtMs).toISOString()}>{formatDateTime(note.recordedAtMs)}</time></div><h2>{note.title}</h2>
        {onAsk && <div className="research-document-actions"><Button variant="outline" size="sm" onClick={() => onAsk({ kind: "research", loupistId: location.loupistId, objectId: note.researchId, label: note.title }, note.chatId)}>{t("researchReader.continueChat")}</Button></div>}</header>
      {note.changeSummary.trim() ? <p className="note-change-summary">{note.changeSummary}</p> : null}
      {markdown(bodyWithoutRepeatedTitle(note.title, note.markdown))}
      {note.inputs && <SavedInputs inputs={note.inputs} openReference={openReference} markdown={markdown} />}
      {Boolean(note.previousNoteIds?.length) && <section aria-label={t("researchReader.previousNotes")}><h3>{t("researchReader.earlierNotesUsed")}</h3><ul>{[...new Set(note.previousNoteIds)].map((id, index) => <li key={id}><Button variant="link" onClick={() => openReference({ kind: "research", recordId: "", revisionId: "", objectId: id })}>{t("researchReader.earlierNote", { index: formatNumber(index + 1) })}</Button></li>)}</ul></section>}
    </article>}
    {journal && <article ref={readerPanel} tabIndex={-1} className="knowledge-reading knowledge-journal" aria-label={t("researchReader.recordedJournal")}>
      <header className="research-document-head"><div className="memory-meta-row"><Badge variant="outline">{t("journal.entry")}</Badge><time dateTime={new Date(journal.createdAtMs).toISOString()}>{formatDateTime(journal.createdAtMs)}</time></div><h2>{journal.judgment}</h2></header>
      {editing ? <ResearchJournalForm bridge={bridge} loupistId={location.loupistId} entry={journal} onCancel={() => setEditing(false)} onSaved={entry => { setJournal(entry); setEditing(false); onChanged?.(); }} /> : <>
        <JournalThenNow entry={journal} onOpenSource={(source) => openReference(sourceReference(source))} />
        {journal.reasoning ? <section aria-label={t("journal.why")}><h3>{t("journal.why")}</h3>{markdown(journal.reasoning)}</section> : null}
        <ResearchJournalCheck entry={journal} />
        {journal.entities.length ? <section aria-label={t("journal.entities")}><h3>{t("journal.entities")}</h3>{journal.entities.map(id => { const world = journal.inputs.worlds.find(w => w.nodes.some(n => n.id === id)); const node = world?.nodes.find(n => n.id === id); return <Button key={id} variant="link" onClick={() => onOpen({ loupistId: location.loupistId, surface: "world", recordId: world?.recordId, revisionId: world?.revisionId, selection: { kind: "node", id } })}>{node?.name ?? id}</Button>; })}</section> : null}
        {journal.originChatId && (onAsk || onOpenChat) ? <Button className="journal-origin-chat" variant="link" onClick={() => { if (onOpenChat) onOpenChat(journal.originChatId!); else onAsk?.({ kind: "journal", loupistId: location.loupistId, objectId: journal.journalId, label: journal.judgment }, journal.originChatId!); }}>{t("journal.openChat")}</Button> : null}
        <div className="journal-actions"><Button variant="outline" onClick={() => setEditing(true)}>{t("journal.edit")}</Button><Button variant="ghost" disabled={archiving} onClick={() => { setArchiving(true); void bridge.journalArchive({ loupistId: location.loupistId, journalId: journal.journalId, archived: !journal.archived }).then(() => { onChanged?.(); onOpen({ loupistId: location.loupistId, surface: "note" }); }).catch(() => setError(t("journal.saveError"))).finally(() => setArchiving(false)); }}>{t(journal.archived ? "journal.restore" : "journal.archive")}</Button></div>
      </>}
      <SavedInputs inputs={journal.inputs} openReference={openReference} markdown={markdown} onReadReceipt={(id, trigger) => void readReceipt(id, trigger)} openingReceipt={openingReceipt ? requestedReceipt : null} />
      {openingReceipt && <p role="status">{t("researchReader.openingTheSourceSavedWithThisJournal")}</p>}
      {receiptError && <div role="alert"><p>{receiptError}</p><Button variant="outline" onClick={() => { if (requestedReceipt) void readReceipt(requestedReceipt); }}>{t("researchReader.retrySavedSource")}</Button></div>}
      {receipt && <article ref={receiptPanel} tabIndex={-1} className="knowledge-journal-source" aria-label={t("researchReader.retainedJournalSource")}>
        <header className="knowledge-read-head"><h3>{t("researchReader.sourceSavedWithThisJournal")}</h3><Button variant="ghost" onClick={() => { receiptGeneration.current += 1; restoreReceiptFocus.current = true; setReceipt(null); }}>{t("researchReader.closeSavedSource")}</Button></header>
        <p>{receipt.pinned.attribution}</p>
        {receipt.text !== undefined ? <blockquote className="knowledge-passage">{receipt.text}</blockquote> : <p>{t("researchReader.theOriginalFileWasSavedWithThisJournalAText")}</p>}
        <Button variant="link" onClick={() => openReference(sourceReference(receipt.pinned))}>{t("researchReader.openSourceRecord")}</Button>
        <Disclosure title={<>{t("researchReader.fileDetails")}</>}><p>{coverageLabel()[receipt.pinned.coverage] ?? receipt.pinned.coverage}</p></Disclosure>
      </article>}
    </article>}
  </section>;
}

function SavedInputs({ inputs, openReference, markdown, onReadReceipt, openingReceipt }: {
  inputs: JournalInputs;
  openReference: (reference: KnowledgeReference) => void;
  markdown: (text: string) => ReactNode;
  onReadReceipt?: (receiptId: string, trigger: HTMLElement) => void;
  openingReceipt?: string | null;
}) {
  const { t } = useTranslation("knowledge");
  if (!inputs.sources.length && !inputs.worlds.length && !inputs.lenses.length && !inputs.earlier?.length) return null;
  return <section className="research-evidence" aria-label={t("researchReader.evidenceAndContextUsed")}>
    <div className="research-evidence-heading"><h3>{t("researchReader.evidenceAndContext")}</h3><p>{t("researchReader.theVersionsSavedWithThisRecord")}</p></div>
    {inputs.sources.length > 0 && <Disclosure className="research-evidence-group" defaultOpen title={<>{t("researchReader.sources")} <span>{formatNumber(inputs.sources.length)}</span></>}>
    <ul>{inputs.sources.map((source, index) => <li key={source.receiptId}>
      <p>{source.attribution || t("researchReader.sourceNumber", { index: formatNumber(index + 1) })}</p><div className="research-evidence-meta"><span>{coverageLabel()[source.coverage] ?? source.coverage}</span><span>{t("researchReader.publishedDate", { date: timePointValue(source.publicationTime) })}</span></div>
      <div className="research-evidence-actions"><Button variant="link" onClick={() => openReference(sourceReference(source))}>{t("researchReader.openSource", { index: formatNumber(index + 1) })}</Button>
      {onReadReceipt && <Button variant="outline" disabled={Boolean(openingReceipt)} onClick={(event) => onReadReceipt(source.receiptId, event.currentTarget)}>{openingReceipt === source.receiptId ? t("researchReader.openingSource") : t("researchReader.readSavedSource")}</Button>}</div>
    </li>)}</ul></Disclosure>}
    {inputs.worlds.length > 0 && <Disclosure className="research-evidence-group" defaultOpen title={<>World <span>{formatNumber(inputs.worlds.length)}</span></>}>
    <ul>{inputs.worlds.map((world, index) => <li key={`${world.recordId}:${world.revisionId}`}>
      <Button variant="link" onClick={() => openReference({ kind: "world", recordId: world.recordId, revisionId: world.revisionId, objectId: world.recordId })}>{t("researchReader.openWorldVersion", { index: formatNumber(index + 1) })}</Button>
      {world.nodes.length > 0 && <ul>{world.nodes.map((node) => <li key={node.id}><Button variant="link" onClick={() => openReference({ kind: "world_node", recordId: world.recordId, revisionId: world.revisionId, objectId: node.id })}>{node.name}</Button>{node.description && <p>{node.description}</p>}</li>)}</ul>}
      {world.edges.length > 0 && <ul>{world.edges.map((edge) => <li key={edge.id}><Button variant="link" onClick={() => openReference({ kind: "world_claim", recordId: world.recordId, revisionId: world.revisionId, objectId: edge.claim.id })}>{edge.claim.proposition}</Button><Badge variant="outline">{claimStatusLabel(edge.status)}</Badge></li>)}</ul>}
    </li>)}</ul></Disclosure>}
    {inputs.lenses.length > 0 && <Disclosure className="research-evidence-group" defaultOpen title={<>Lens <span>{formatNumber(inputs.lenses.length)}</span></>}>
    <ul>{inputs.lenses.map((lens, index) => <li key={`${lens.recordId}:${lens.revisionId}`}>
      <Button variant="link" onClick={() => openReference({ kind: "lens", recordId: lens.recordId, revisionId: lens.revisionId, objectId: lens.recordId })}>{t("researchReader.openLens", { index: formatNumber(index + 1) })}</Button>
      <p>{approachStatus(lens.status)}</p>
      <Disclosure className="research-saved-lens" title={<>{t("researchReader.savedLensDocument")}</>}>{markdown(lens.document.markdown)}<dl>{Object.entries(lens.document).filter(([key, text]) => key !== "markdown" && text).map(([key, text]) => <div key={key}><dt>{lensFieldLabel(key)}</dt><dd>{markdown(text)}</dd></div>)}</dl></Disclosure>
    </li>)}</ul></Disclosure>}
    {Boolean(inputs.earlier?.length) && <section aria-label={t("researchReader.earlierResearchUsed")}><h3>{t("researchReader.earlierResearchUsed")}</h3><ul>{inputs.earlier?.map((earlier, index) => <li key={`${earlier.researchId}:${earlier.journalId ?? ""}`}>
      <Button variant="link" onClick={() => openReference({ kind: earlier.journalId ? "journal" : "research", recordId: "", revisionId: "", objectId: earlier.journalId ?? earlier.researchId })}>{t(earlier.journalId ? "researchReader.openEarlierJournal" : "researchReader.openEarlierNote", { index: formatNumber(index + 1) })}</Button>
      <p>{earlier.relation === "review" ? t("researchReader.reviewed") : earlier.relation === "supersedes" ? t("researchReader.supersedesTheEarlierInterpretation") : t("researchReader.contradictsTheEarlierInterpretation")}</p>
      <Disclosure title={<>{t("researchReader.earlierTextUsed")}</>}>{markdown(earlier.deliveredText)}<p>{coverageLabel()[earlier.coverage] ?? earlier.coverage}</p></Disclosure>
    </li>)}</ul></section>}
  </section>;
}

const sourceReference = (source: JournalSourceRead["pinned"]): KnowledgeReference => ({ kind: "source", recordId: source.recordId, revisionId: source.revisionId, objectId: source.spanId, spanId: source.spanId, receiptId: source.receiptId });
const coverageLabel = (): Record<string, string> => ({ included_full: i18n.t("knowledge:researchReader.fullDocument"), included_excerpt: i18n.t("knowledge:researchReader.excerpt"), upstream_truncated: i18n.t("knowledge:researchReader.incompleteSourceText"), metadata_only: i18n.t("knowledge:researchReader.sourceDetailsOnly"), omitted: i18n.t("knowledge:researchReader.contentNotSaved") });
const approachStatus = (status: string) => ({ considered: i18n.t("knowledge:researchReader.considered"), applied: i18n.t("knowledge:researchReader.applied"), adapted: i18n.t("knowledge:researchReader.adapted"), rejected: i18n.t("knowledge:researchReader.setAside") }[status] ?? status.replaceAll("_", " "));

const lensFieldLabel = (key: string) => ({ applicability: i18n.t("knowledge:researchReader.applicability"), assumptions: i18n.t("knowledge:researchReader.assumptions"), procedure: i18n.t("knowledge:researchReader.procedure"), principles: i18n.t("knowledge:researchReader.principles"), limitations: i18n.t("knowledge:researchReader.limitations") }[key] ?? key);
