import { formatDate, formatNumber } from "../../i18n/format";
import { useTranslation } from "../../i18n";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ArrowLeftIcon, SearchIcon, XIcon } from "lucide-react";
import type { LoupistBridge, JournalRecord, KnowledgeLocation, KnowledgePin, ResearchRecord } from "../../types";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { EmptyState } from "../ui/empty-state";
import { Spinner } from "../ui/spinner";
import { Input } from "../ui/input";
import { ResearchReader } from "./ResearchReader";
import { journalCounts, ResearchJournalStatus } from "./ResearchJournal";
import { ResearchJournalForm } from "./ResearchJournalForm";

type Props = {
  compact?: boolean; bridge: LoupistBridge; location: KnowledgeLocation; query: string;
  onQueryChange: (query: string) => void; refreshKey?: number;
  onOpen: (location: KnowledgeLocation) => void; onPatch: (location: KnowledgeLocation) => void;
  onAsk: (pin: KnowledgePin, threadId?: string) => void;
  onChanged?: () => void;
};

/** Journal entries and the Loupist's research notes share one Library tab. */
export function ResearchLibrary({ bridge, location, query, onQueryChange, refreshKey = 0, compact = false, onOpen, onPatch, onAsk, onChanged }: Props) {
  const { t } = useTranslation("knowledge");
  const [records, setRecords] = useState<ResearchRecord[]>([]);
  const [journals, setJournals] = useState<JournalRecord[]>([]);
  const [creating, setCreating] = useState(Boolean(location.view?.journalNew));
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const libraryRef = useRef<HTMLDivElement>(null);
  const readerRef = useRef<HTMLDivElement>(null);
  const restoreLibraryFocus = useRef(false);
  const patchView = (view: NonNullable<KnowledgeLocation["view"]>) => onPatch({ ...location, view: { ...location.view, ...view } });
  const selectedId = location.selection?.kind === "research" ? location.selection.recordId : location.selection?.kind === "journal" ? location.selection.journalId : undefined;
  useEffect(() => {
    let active = true;
    setLoading(true); setError(null);
    const timer = setTimeout(() => {
      void Promise.all([bridge.knowledgeResearchSearch({ loupistId: location.loupistId, query: query.trim() || undefined }), bridge.journalList({ loupistId: location.loupistId })]).then(([page, entries]) => {
        if (!active) return;
        if (page.loupistId !== location.loupistId) throw new Error(t("researchLibrary.theRequestedLoupistSResearchWasNotReturned"));
        setRecords(page.records.filter(record => !record.journal)); setJournals(entries);
      }).catch((cause: unknown) => { if (active) setError(errorMessage(cause, t("researchLibrary.couldNotLoadSavedResearch"))); })
        .finally(() => { if (active) setLoading(false); });
    }, query ? 180 : 0);
    return () => { active = false; clearTimeout(timer); };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `t` only words the failure; a language change must not reload.
  }, [bridge, location.loupistId, query, refreshKey, reload]);
  useLayoutEffect(() => {
    if (!loading && libraryRef.current) {
      libraryRef.current.scrollTop = location.view?.researchLibraryScrollTop ?? 0;
      if (!selectedId && restoreLibraryFocus.current) {
        restoreLibraryFocus.current = false;
        const row = [...libraryRef.current.querySelectorAll<HTMLElement>("[data-research-id]")].find(item => item.dataset.researchId === location.view?.focusResearchId);
        (row ?? libraryRef.current.querySelector<HTMLInputElement>("input"))?.focus({ preventScroll: true });
      }
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- restores scroll and focus when the list loads or the selection changes, not on every view patch.
  }, [loading, selectedId, location.loupistId]);
  const openJournal = (entry: JournalRecord) => onOpen({ loupistId: location.loupistId, surface: "note", objectId: entry.journalId, selection: { kind: "journal", journalId: entry.journalId }, view: { ...location.view, readerScrollTop: 0, focusResearchId: entry.journalId } });
  const counts = journalCounts(journals);
  const matches = journals.filter(entry => `${entry.judgment} ${entry.reasoning}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  const changed = () => { setReload(value => value + 1); onChanged?.(); };
  return <div className={`research-workspace${selectedId && !compact ? " has-reader" : ""}${compact ? " is-library-only" : ""}`}>
    <div ref={libraryRef} className="knowledge-browser research-library" onScroll={event => patchView({ researchLibraryScrollTop: event.currentTarget.scrollTop })}>
      <div className="research-library-controls"><div className="memory-search"><SearchIcon aria-hidden="true" /><Input aria-label={t("journal.search")} placeholder={t("journal.searchPlaceholder")} value={query} onChange={event => onQueryChange(event.target.value)} />{query ? <Button variant="ghost" size="icon-sm" aria-label={t("researchLibrary.clearSearch")} onClick={() => onQueryChange("")}><XIcon /></Button> : null}</div></div>
      {error ? <div role="alert"><p>{error}</p><Button variant="outline" onClick={changed}>{t("researchLibrary.retryResearch")}</Button></div> : null}
      <section className="journal-library-section" aria-label={t("journal.yourJournal")}>
        <div className="source-library-heading library-section-label"><h2>{t("journal.yourJournal")}</h2><span>{formatNumber(matches.length)}</span>{journals.length > 0 ? <p className="journal-summary">{t("journal.summary", { cameTrue: formatNumber(counts.cameTrue), didNot: formatNumber(counts.didNot), pending: formatNumber(counts.pending) })}{counts.unclear ? ` · ${t("journal.unclearCount", { count: counts.unclear, value: formatNumber(counts.unclear) })}` : ""}</p> : null}<Button variant="ghost" className="library-add" onClick={() => setCreating(true)}>{t("journal.newEntry")}</Button></div>
        {creating ? <ResearchJournalForm bridge={bridge} loupistId={location.loupistId} checkOpen={location.view?.journalCheckOpen} onCancel={() => setCreating(false)} onSaved={entry => { setCreating(false); changed(); openJournal(entry); }} /> : null}
        {loading ? <p className="memory-loading-inline" role="status"><Spinner />{t("journal.loading")}</p> : !error && !matches.length && !creating ? <p className="journal-caption">{t(query ? "journal.noMatches" : "journal.empty")}</p> : null}
        <ul className="journal-entry-list">{matches.map(entry => <li key={entry.journalId}><Button variant="ghost" className="research-library-row journal-library-row" data-testid="journal-library-row" data-research-id={entry.journalId} aria-current={entry.journalId === selectedId ? "true" : undefined} onClick={() => openJournal(entry)}><span className="memory-rail-copy"><strong className="journal-judgment-clamp">{entry.judgment}</strong><small><time dateTime={new Date(entry.createdAtMs).toISOString()}>{formatDate(entry.createdAtMs)}</time>{entry.check ? <><span aria-hidden="true"> · </span><ResearchJournalStatus entry={entry} /></> : null}</small></span></Button></li>)}</ul>
      </section>
      <section className="research-notes-section" aria-label={t("journal.researchNotes")}><div className="source-library-heading library-section-label"><h2>{t("journal.researchNotes")}</h2><span>{formatNumber(records.length)}</span></div>
        {!loading && !error && !records.length ? <EmptyState title={t(query ? "researchLibrary.noMatchingResearch" : "researchLibrary.noResearchNotesYet")} description={t("journal.researchEmpty")} /> : null}
        <ul aria-label={t("researchLibrary.savedResearch")}>{records.map(record => <li key={record.id}><Button variant="ghost" className="research-library-row" data-research-id={record.id} aria-current={record.id === selectedId ? "true" : undefined} onClick={() => onOpen({ loupistId: location.loupistId, surface: "note", objectId: record.id, selection: { kind: "research", recordId: record.id }, view: { ...location.view, readerScrollTop: 0, focusResearchId: record.id } })}><span className="memory-rail-copy"><strong>{record.title || t("journal.researchNotes")}</strong>{record.summary && record.summary !== record.title ? <span className="source-summary">{record.summary}</span> : null}<small><time dateTime={record.recordedAt}>{Number.isNaN(Date.parse(record.recordedAt)) ? record.recordedAt : formatDate(new Date(record.recordedAt))}</time></small></span></Button></li>)}</ul>
      </section>
    </div>
    {selectedId && !compact ? <div ref={readerRef} className="research-reader-pane" onScroll={event => patchView({ readerScrollTop: event.currentTarget.scrollTop })}><div className="research-reader-toolbar"><Button variant="ghost" size="sm" aria-label={t("researchLibrary.closeRecord")} onClick={() => { restoreLibraryFocus.current = true; onPatch({ loupistId: location.loupistId, surface: "note", view: { ...location.view, readerScrollTop: 0 } }); }}><ArrowLeftIcon />{t("journal.backToNotes")}</Button></div><ResearchReader bridge={bridge} location={location} onOpen={onOpen} onAsk={onAsk} refreshKey={refreshKey} onChanged={changed} onReady={() => { if (readerRef.current) readerRef.current.scrollTop = location.view?.readerScrollTop ?? 0; }} /></div> : null}
  </div>;
}
