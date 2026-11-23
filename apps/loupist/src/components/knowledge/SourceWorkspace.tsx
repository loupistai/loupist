import { SavedPassage } from "./SavedPassage";
import { readWithWorldRetry } from "../../knowledge/readWithWorldRetry";
import { SavedSourceData } from "./SavedSourceData";
import { revealTableCell } from "./revealTableCell";
import { readableCitedPassages } from "./citedPassages";
import { QuoteAsk } from "../chat/QuoteAsk";
import { useSettledScroll } from "./settledScroll";
import { ConnectionOrigin } from "../connections/custom/ConnectionOrigin";
import { formatDate, formatNumber } from "../../i18n/format";
import { coverageLabel, metadataLabel, dateValue, timePointValue, spanSelectorLabel } from "../../knowledge/presentation";
import i18n, { useTranslation } from "../../i18n";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { Disclosure } from "@/components/ui/disclosure";
import { Fragment, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ArrowLeftIcon, CheckIcon, ChevronDownIcon, ChevronUpIcon, ExternalLinkIcon, FileTextIcon, SearchIcon, Settings2Icon, UploadIcon, XIcon } from "lucide-react";
import { Button } from "../ui/button";
import { ExternalLinkError, useExternalLink } from "../navigation/ExternalLinkError";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";
import { Badge } from "../ui/badge";
import { Alert, AlertDescription } from "../ui/alert";
import { EmptyState } from "../ui/empty-state";
import { Spinner } from "../ui/spinner";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import type { LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin, KnowledgeSourceFile, KnowledgeSourceSelectionRequest, RepresentationRole, SourceClass, SourceDetail } from "../../types";
import { errorMessage } from "../../lib/errors";
import { pinForSource, type SourceOpen } from "../../knowledge/location";
import { compactOrigin, sourceOrigin, sourceTitle } from "../../knowledge/hostSource";
import { isMapProcessing, processingSourceCount, sourceDisplayStatus, sourceProcessingLabel, type WorldProcessing } from "../../knowledge/mapProcessing";
import { SegmentedControl, SegmentedControlItem } from "../ui/segmented-control";
import { relationName } from "../map/relations";
import { SourceRetry } from "./SourceRetry";

const originLabel = (): Record<SourceClass, string> => ({
  publisher_document: i18n.t("knowledge:sourceWorkspace.publisherDocument"), dataset: i18n.t("knowledge:sourceWorkspace.dataset"), user_statement: i18n.t("knowledge:sourceWorkspace.userStatement"),
  tool_result: i18n.t("knowledge:sourceWorkspace.toolResult"), analysis: i18n.t("knowledge:sourceWorkspace.analysis"), imported_contributor: i18n.t("knowledge:sourceWorkspace.importedSource"),
  code_revision: i18n.t("knowledge:sourceWorkspace.sourceCode"), web_page: i18n.t("knowledge:sourceWorkspace.webPage"), historical_document: i18n.t("knowledge:sourceWorkspace.historicalDocument"), user_file: i18n.t("knowledge:sourceWorkspace.userFile"),
  user_provided: i18n.t("knowledge:sourceWorkspace.userProvided"),
});
const materialLabel = (): Record<RepresentationRole, string> => ({ original: i18n.t("knowledge:sourceWorkspace.originalMaterial"), provider_payload: i18n.t("knowledge:sourceWorkspace.downloadedCopy"), extracted_text: i18n.t("knowledge:sourceWorkspace.extractedText"), structured: i18n.t("knowledge:sourceWorkspace.structuredData"), excerpt: i18n.t("knowledge:sourceWorkspace.excerpt"), derived: i18n.t("knowledge:sourceWorkspace.processedMaterial") });
const sourceDate = (value: string | undefined) => {
  if (!value) return value;
  if (/^\d{4}(?:-\d{2})?$/.test(value)) return dateValue(value);
  // A publication day is a local calendar date, not a UTC instant.
  const date = new Date(/^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T00:00:00` : value);
  // Korean reads the long form (2026년 10월 27일), not "2026. 10. 27.".
  return Number.isNaN(date.getTime()) ? value : formatDate(date, { dateStyle: i18n.language.toLowerCase().startsWith("ko") ? "long" : "medium" });
};
const textMedia = (value: string) => value.startsWith("text/") || value === "application/json";

/** A row shows its Map status only when it says more than "Source saved": a finished or unprocessed source stays quiet. */
const quietStatus = (status?: string) => status === "completed" || sourceProcessingLabel(status) === sourceProcessingLabel();

/** Where a user-provided Source came from: the Chat it was pasted in and what the user said about it, none of it verified. */
function ProvidedNote({ provided }: { provided: NonNullable<SourceDetail["provided"]> }) {
  const { t } = useTranslation("knowledge");
  const pasted = sourceDate(new Date(provided.pastedAtMs).toISOString());
  return <p className="knowledge-hint source-provided" data-testid="source-provided">
    {t("sourceWorkspace.pastedInChat", { date: pasted })}
    {provided.statedPublisher ? <> · {t("sourceWorkspace.statedPublisher", { publisher: provided.statedPublisher })}</> : null}
    {" "}{t("sourceWorkspace.statedNotVerified")}
  </p>;
}

type Props = { bridge: LoupistBridge; catalog: KnowledgeCatalog; worldProcessing?: WorldProcessing; location: KnowledgeLocation; sourceOpen?: SourceOpen; query?: string; onQueryChange?: (query: string) => void; onManageProcessing?: () => void; onManageProviders?: () => void; onAddSource?: () => void; readerOnly?: boolean; compact?: boolean; onOpen: (location: KnowledgeLocation) => void; onPatch?: (location: KnowledgeLocation) => void; onAsk: (pin: KnowledgePin) => void; onChanged: () => void; onOpenLink?: (url: string) => void | Promise<void>; opensLinksInPanel?: boolean; quoteCitedSelections?: boolean };

export function SourceWorkspace({ bridge, catalog, worldProcessing, location, sourceOpen, query = "", onQueryChange, onManageProcessing, onManageProviders, onAddSource, readerOnly = false, compact = false, onOpen, onPatch = onOpen, onAsk, onChanged, onOpenLink, opensLinksInPanel, quoteCitedSelections }: Props) {
  const { t } = useTranslation("knowledge");
  const selected = location.selection?.kind === "source" ? location.selection : undefined;
  const sourceRecord = catalog.sources.find((item) => item.recordId === location.recordId);
  const [detail, setDetail] = useState<SourceDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [processing, setProcessing] = useState<string | undefined>();
  const [detailRefreshError, setDetailRefreshError] = useState<string | null>(null);
  const [detailRefreshing, setDetailRefreshing] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [exported, setExported] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  const [wholeSavedText, setWholeSavedText] = useState(false);
  const [passageIndex, setPassageIndex] = useState<number | null>(null);
  const originalLink = useExternalLink(url => onOpenLink ? onOpenLink(url) : bridge.openExternalUrl(url));
  const opensOriginalInPanel = opensLinksInPanel && Boolean(onOpenLink);
  const generation = useRef(0);
  const readerRef = useRef<HTMLElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);
  const libraryRef = useRef<HTMLDivElement | null>(null);
  const scrollReaderRef = useRef<HTMLDivElement | null>(null);
  const readerToolbarRef = useRef<HTMLDivElement | null>(null);
  const filter = location.view?.sourceFilter ?? "all";
  const patchView = (view: KnowledgeLocation["view"]) => onPatch({ ...location, view: { ...location.view, ...view } });
  const changedRef = useRef(onChanged);
  changedRef.current = onChanged;
  const scopeKey = [location.loupistId, location.recordId, location.revisionId, selected?.sourceId, selected?.spanId, selected?.worldRecordId, selected?.worldRevisionId].join(":");
  const currentScope = useRef(scopeKey);
  currentScope.current = scopeKey;
  const reading = !compact && Boolean(selected?.sourceId || selected?.spanId);
  const span = detail?.spans.find((item) => item.spanId === detail.selectedSpanId) ?? detail?.spans[0];
  const presentation = span?.presentation;
  const readableText = presentation?.text ?? span?.text;
  const savedTable = presentation?.table;
  const focusTable = Boolean(savedTable?.focusedCell && !detail?.figureFailure);
  const mediaType = span?.mediaType;
  const focusedMark = detail?.figureFailure ? undefined : presentation?.focusedMark;
  const passages = useMemo(() => !savedTable && presentation && mediaType && textMedia(mediaType)
    ? readableCitedPassages(presentation.text, presentation.citedPassages, focusedMark) : [], [presentation, mediaType, focusedMark, savedTable]);
  const hasCitedPassages = passages.length > 0;
  const readingCitations = hasCitedPassages && !wholeSavedText;
  const initialPassage = passages.findIndex(passage => passage.marks.some(mark => mark.focused));
  const currentPassage = Math.min(passageIndex ?? Math.max(0, initialPassage), passages.length - 1);
  const stepPassage = (delta: -1 | 1) => {
    setPassageIndex(currentPassage + delta);
    setWholeSavedText(false);
  };
  const onLibraryScroll = useSettledScroll((top) => patchView({ sourceScrollTop: top }), scopeKey, compact);
  const onReaderScroll = useSettledScroll((top) => patchView({ readerScrollTop: top }), scopeKey);
  const readSelectedSource = useCallback(() => bridge.knowledgeSourceRead({
    loupistId: location.loupistId, recordId: location.recordId!, revisionId: location.revisionId!,
    sourceId: selected?.sourceId, spanId: selected?.spanId, worldRecordId: selected?.worldRecordId,
    worldRevisionId: selected?.worldRevisionId, purpose: "renderer_inspection",
    ...(sourceOpen?.figure ? { figure: sourceOpen.figure } : {}),
  }), [bridge, location.loupistId, location.recordId, location.revisionId, selected?.sourceId, selected?.spanId, selected?.worldRecordId, selected?.worldRevisionId, sourceOpen]);

  useEffect(() => {
    const ticket = ++generation.current;
    setWholeSavedText(false); setPassageIndex(null);
    setDetail(null); setError(null); setProcessing(undefined); setDetailRefreshError(null); setDetailRefreshing(false); setExporting(false); setExported(false); setExportError(null);
    if (!reading || !location.recordId || !location.revisionId) return;
    void readWithWorldRetry(readSelectedSource, () => generation.current === ticket)
      .then((next) => { if (generation.current === ticket) { setDetail(next); setProcessing(next.processing?.status); } })
      .catch((cause: unknown) => { if (generation.current === ticket) setError(errorMessage(cause, t("sourceWorkspace.couldNotOpenThisSource"))); });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a request counter, not a DOM node: cleanup invalidates the request that is in flight.
    return () => { generation.current++; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `readSelectedSource` names the record and revision; `t` only words the failure; a language change must not reload.
  }, [readSelectedSource, reload, reading]);

  const refreshDetail = useCallback(async () => {
    const ticket = generation.current;
    const captured = scopeKey;
    setDetailRefreshing(true);
    setDetailRefreshError(null);
    try {
      const next = await readWithWorldRetry(readSelectedSource, () => generation.current === ticket);
      if (generation.current !== ticket || currentScope.current !== captured) return;
      setDetail(next);
      setProcessing(next.processing?.status);
    } catch (cause: unknown) {
      if (generation.current !== ticket || currentScope.current !== captured) return;
      setDetailRefreshError(errorMessage(cause, t("sourceWorkspace.couldNotRefreshSourceDetails")));
    } finally {
      if (generation.current === ticket && currentScope.current === captured) setDetailRefreshing(false);
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `t` only words the failure; a language change must not restart the processing poll.
  }, [readSelectedSource, scopeKey]);

  useEffect(() => {
    if (!detail) return;
    readerRef.current?.focus({ preventScroll: true });
    if (!hasCitedPassages && !focusTable && scrollReaderRef.current) scrollReaderRef.current.scrollTop = location.view?.readerScrollTop ?? 0;
  // eslint-disable-next-line react-hooks/exhaustive-deps -- focuses and restores the reader once per opened source span.
  }, [detail?.sourceId, detail?.selectedSpanId]);

  useLayoutEffect(() => {
    const scroller = scrollReaderRef.current;
    if (!hasCitedPassages || !scroller) return;
    if (wholeSavedText) {
      scroller.scrollTop = 0;
      return;
    }
    const passage = scroller.querySelector<HTMLElement>(`[data-cited-passage="${currentPassage}"]`);
    if (!passage) return;
    const toolbarHeight = readerToolbarRef.current?.offsetHeight ?? 0;
    const focused = passage.querySelector("mark[data-cited-focus]");
    const target = passage.offsetHeight > scroller.clientHeight - toolbarHeight && focused ? focused : passage;
    scroller.scrollTop = Math.max(0, scroller.scrollTop + target.getBoundingClientRect().top
      - scroller.getBoundingClientRect().top - toolbarHeight);
    // A cited figure in a wide table also comes into view sideways, beside its row's label.
    const cited = focused ?? passage.querySelector("mark.evidence-match");
    if (cited) revealTableCell(cited);
  }, [detail?.sourceId, span?.spanId, hasCitedPassages, currentPassage, wholeSavedText]);

  useLayoutEffect(() => {
    if (libraryRef.current) libraryRef.current.scrollTop = location.view?.sourceScrollTop ?? 0;
    if (!reading && location.view?.focusSourceId) {
      const row = [...(listRef.current?.querySelectorAll<HTMLButtonElement>("button[data-source-id]") ?? [])].find((button) => button.dataset.sourceId === location.view?.focusSourceId);
      (row ?? libraryRef.current?.querySelector<HTMLInputElement>("input"))?.focus({ preventScroll: true });
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- restores the list once per Loupist, mode and filter.
  }, [reading, location.loupistId, filter]);

  useEffect(() => {
    const jobId = detail?.processing?.jobId;
    if (!jobId || (processing !== "queued" && processing !== "running")) return;
    let cancelled = false;
    let timer = 0;
    const captured = scopeKey;
    const poll = () => {
      timer = window.setTimeout(() => {
        void bridge.knowledgeGraphJob({ loupistId: location.loupistId, jobId }).then((job) => {
          if (cancelled || currentScope.current !== captured) return;
          setProcessing(job.status);
          if (job.status === "queued" || job.status === "running") poll();
          else { changedRef.current(); void refreshDetail(); }
        }).catch(() => { if (!cancelled && currentScope.current === captured) poll(); });
      }, 2500);
    };
    poll();
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [bridge, detail?.processing?.jobId, location.loupistId, processing, refreshDetail, scopeKey]);

  // Jobs a row's "Try again" started: followed until they settle, then the catalog refreshes.
  const [followedJobs, setFollowedJobs] = useState<readonly string[]>([]);
  useEffect(() => setFollowedJobs([]), [location.loupistId]);
  useEffect(() => {
    if (!followedJobs.length) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void Promise.all(followedJobs.map((jobId) => bridge.knowledgeGraphJob({ loupistId: location.loupistId, jobId }).then((job) => job.status, () => "queued"))).then((statuses) => {
        if (cancelled) return;
        const still = followedJobs.filter((_, index) => statuses[index] === "queued" || statuses[index] === "running");
        if (still.length !== followedJobs.length) changedRef.current();
        setFollowedJobs(still);
      });
    }, 2500);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [bridge, followedJobs, location.loupistId]);
  const retrySource = (recordId: string) => () => bridge.knowledgeGraphRetrySource({ loupistId: location.loupistId, recordId });
  const rowRetried = (job: { jobId: string; status: string }) => {
    changedRef.current();
    if (job.status === "queued" || job.status === "running") setFollowedJobs((jobs) => jobs.includes(job.jobId) ? jobs : [...jobs, job.jobId]);
  };
  const readerRetried = (job: { status: string }) => {
    setProcessing(job.status);
    changedRef.current();
    void refreshDetail();
  };

  const search = query.normalize("NFKC").trim().toLowerCase();
  const readiness = worldProcessing?.readiness;
  const activity = worldProcessing?.activity;
  const waiting = activity?.kind === "waiting" ? activity.reason : undefined;
  const buildingWithoutSources = activity?.kind === "building" && activity.sourceCount === 0;
  const showQueueStatus = waiting || buildingWithoutSources;
  const queuedStatus = sourceDisplayStatus("queued", readiness);
  const matching = catalog.sources.flatMap(record => record.objects.map(item => ({
    record, item, status: sourceDisplayStatus(item.processing?.status, readiness),
  }))).filter(({ item }) => !search || `${sourceTitle(item)} ${item.summary ?? ""} ${sourceOrigin(item) ?? ""}`.normalize("NFKC").toLowerCase().includes(search));
  const processingCount = processingSourceCount(catalog, readiness);
  const needsAttention = (status?: string) => (status !== undefined && status === queuedStatus && !isMapProcessing(status))
    || ["paused", "paused_budget", "failed_before_mutation", "reconcile_required", "cancelled_before_start", "unsupported", "no_text"].includes(status ?? "");
  const sources = matching.filter(({ status }) => filter === "all" || (filter === "processing" ? isMapProcessing(status) : needsAttention(status)));
  const readerProcessing = sourceDisplayStatus(processing, readiness);
  const processingSettings = onManageProcessing ? <Button variant="ghost" size="icon-sm" aria-label={t("sourceWorkspace.worldProcessingSettings")} title={t("sourceWorkspace.worldProcessingSettings")} onClick={onManageProcessing}><Settings2Icon /></Button> : null;
  // With no AI account at all, the control opens AI accounts instead of World building settings.
  const statusControl = (status?: string) => status === "no_account"
    ? onManageProviders ? <Button variant="ghost" size="xs" onClick={onManageProviders}>{t("worldProcessingSettings.manageProviders")}</Button> : null
    : processingSettings;
  const originalUrl = (() => { try { const url = new URL(detail?.originLabel ?? ""); return url.protocol === "https:" || url.protocol === "http:" ? url.href : null; } catch { return null; } })();
  const retainedSource = readerOnly && detail && span ? {
    loupistId: location.loupistId, recordId: detail.recordId, revisionId: detail.revisionId,
    sourceId: detail.sourceId, spanId: span.spanId, worldRecordId: selected?.worldRecordId,
    worldRevisionId: selected?.worldRevisionId, purpose: "renderer_inspection",
  } : undefined;
  const quoteAttributes = retainedSource ? {
    "data-quote-source": `source:${retainedSource.sourceId}:${retainedSource.spanId}`,
    "data-quote-retained-source": JSON.stringify(retainedSource),
  } : {};
  const addSelectionToQuestion = async (_sourceId: string, _text: string, request?: KnowledgeSourceSelectionRequest) => {
    if (!request) return;
    const ticket = generation.current;
    const pin = await bridge.knowledgeSourceSelectionResolve(request);
    if (generation.current === ticket) onAsk(pin);
  };
  const exportFile = async () => {
    if (!detail || !span || exporting) return;
    const captured = scopeKey;
    setExporting(true); setExported(false); setExportError(null);
    try {
      const path = await bridge.knowledgeSourceExport({ loupistId: location.loupistId, recordId: detail.recordId, revisionId: detail.revisionId, sourceId: detail.sourceId, spanId: span.spanId, worldRecordId: selected?.worldRecordId, worldRevisionId: selected?.worldRevisionId, purpose: "renderer_inspection" });
      if (currentScope.current === captured) setExported(Boolean(path));
    } catch (cause) { if (currentScope.current === captured) setExportError(errorMessage(cause, t("sourceWorkspace.couldNotExportThisSource"))); }
    finally { if (currentScope.current === captured) setExporting(false); }
  };
  const closeReader = () => {
    onPatch({ loupistId: location.loupistId, surface: "source", view: { ...location.view, readerScrollTop: 0, focusSourceId: selected?.sourceId ?? detail?.sourceId } });
  };
  const claimGroups = detail ? [
    { kind: "facts", label: t("sourceWorkspace.factsFromThisSource"), claims: detail.relatedClaims.filter(claim => !claim.isRelationship) },
    { kind: "relationships", label: t("sourceWorkspace.relationships"), claims: detail.relatedClaims.filter(claim => claim.isRelationship) },
  ] : [];

  return <div className={`knowledge-source world-sources${reading ? " has-reader" : ""}${readerOnly ? " source-reader-only" : ""}${compact ? " is-library-only" : ""}`}>
    {!readerOnly ? <div ref={libraryRef} className="knowledge-browser source-library" onScroll={onLibraryScroll}>
      <div className="source-library-controls">
      {compact ? null : <div className="source-library-heading"><h2>{t("sourceWorkspace.savedSources")}</h2><span>{formatNumber(sources.length)}</span></div>}
      {onQueryChange ? <div className="memory-search source-search"><SearchIcon aria-hidden="true" /><Input aria-label={t("sourceWorkspace.searchSources")} placeholder={t("sourceWorkspace.companyFilingOrSource")} value={query} onChange={(event) => onQueryChange(event.target.value)} />{query ? <Button variant="ghost" size="icon-sm" aria-label={t("sourceWorkspace.clearSearch")} onClick={() => onQueryChange("")}><XIcon /></Button> : null}</div> : null}
      <SegmentedControl value={filter} onValueChange={(value) => patchView({ sourceFilter: value as "all" | "processing" | "attention", sourceScrollTop: 0 })} aria-label={t("sourceWorkspace.sourceProcessingFilters")}><SegmentedControlItem value="all">{t("sourceWorkspace.all")}</SegmentedControlItem><SegmentedControlItem value="processing">{processingCount ? t("sourceWorkspace.processingCount", { countText: formatNumber(processingCount) }) : t("sourceWorkspace.processing")}</SegmentedControlItem><SegmentedControlItem value="attention">{t("sourceWorkspace.needsAttention")}</SegmentedControlItem></SegmentedControl></div>
      {compact ? <div className="source-library-heading library-section-label"><h2>{t("sourceWorkspace.savedSources")}</h2><span>{formatNumber(sources.length)}</span>{onAddSource ? <Button variant="outline" size="sm" className="library-add" onClick={onAddSource}>{t("map.addInLibrary")}</Button> : null}</div> : null}
      {showQueueStatus ? <div className="source-processing-status">
        <p role="status">{buildingWithoutSources ? <Spinner /> : null}{sourceProcessingLabel(waiting ?? "running")}</p>
        {waiting ? statusControl(waiting) : null}
      </div> : null}
      {!sources.length ? <EmptyState title={search || filter !== "all" ? t("sourceWorkspace.noMatchingSources") : t("sourceWorkspace.noSourcesYet")} description={search || filter !== "all" ? t("sourceWorkspace.tryAnotherSearchOrChooseAll") : t("sourceWorkspace.addTextAFileOrALinkToUseIn")} /> : null}
      <ul ref={listRef} aria-label={t("sourceWorkspace.sources")}>
        {sources.map(({ record, item, status }) => <li key={`${record.recordId}:${item.id}`}>
          <Button variant="ghost" className="source-library-row" data-testid="source-library-row" data-source-id={item.id} aria-current={item.id === selected?.sourceId} onClick={() => onOpen({ loupistId: location.loupistId, surface: "source", recordId: record.recordId, revisionId: record.revisionId, objectId: item.id, view: { ...location.view, readerScrollTop: 0, focusSourceId: item.id }, selection: { kind: "source", sourceId: item.id } })}>
            <span className="memory-rail-copy"><strong>{sourceTitle(item)}</strong>{item.summary ? <span className="source-summary">{item.summary}</span> : null}
              <small className="source-row-meta"><ConnectionOrigin origin={item.origin} />{item.userProvided ? <span className="source-row-provided">{t("sourceWorkspace.userProvided")}</span> : null}<span className="source-row-origin" title={sourceOrigin(item)}>{compactOrigin(sourceOrigin(item))}</span>{item.publicationTime?.value ?? item.acquiredAt?.value ? <time title={item.publicationTime?.value ?? item.acquiredAt?.value}>{sourceDate(item.publicationTime?.value ?? item.acquiredAt?.value)}</time> : null}
              {quietStatus(status) || (status === waiting && status !== item.processing?.status) ? null : <span className="source-row-status" data-attention={needsAttention(status) || undefined}>{isMapProcessing(status) ? <Spinner /> : status === "completed" ? <CheckIcon aria-hidden="true" /> : null}{sourceProcessingLabel(status)}</span>}</small>
            </span>
          </Button>
          {item.processing?.status === "paused_budget" ? <SourceRetry key={item.processing.jobId ?? "retry"} retry={retrySource(record.recordId)} onRetried={rowRetried} /> : null}
        </li>)}
      </ul>
    </div> : null}
    {reading ? <div ref={scrollReaderRef} className="knowledge-reading source-reader" onScroll={onReaderScroll}>
      <div ref={readerToolbarRef} className={hasCitedPassages ? "memory-reader-toolbar source-reader-toolbar" : "memory-reader-toolbar"}>
        {!readerOnly ? <Button variant="ghost" size="sm" aria-label={t("sourceWorkspace.closeSource")} onClick={closeReader}><ArrowLeftIcon />{t("sourceWorkspace.backToSources")}</Button> : <span>{t("sourceWorkspace.savedSourceReadOnly")}</span>}
        {detail ? <Button size="sm" onClick={() => onAsk(pinForSource(detail, location.loupistId))}>{t("sourceWorkspace.addToQuestion")}</Button> : null}
        {hasCitedPassages ? <div className="source-passage-controls">
          <Button variant="outline" size="sm" onClick={() => setWholeSavedText(value => !value)}>
            {t(wholeSavedText ? "sourceWorkspace.showCitedPassages" : "sourceWorkspace.showWholeSavedText")}
          </Button>
          {passages.length > 1 ? <div className="source-passage-steps">
            <Button variant="ghost" size="icon-sm" aria-label={t("sourceWorkspace.previousCitedPassage")} title={t("sourceWorkspace.previousCitedPassage")} disabled={currentPassage === 0} onClick={() => stepPassage(-1)}><ChevronUpIcon /></Button>
            <span aria-live="polite">{t("sourceWorkspace.citedPassagePosition", { position: formatNumber(currentPassage + 1), total: formatNumber(passages.length) })}</span>
            <Button variant="ghost" size="icon-sm" aria-label={t("sourceWorkspace.nextCitedPassage")} title={t("sourceWorkspace.nextCitedPassage")} disabled={currentPassage === passages.length - 1} onClick={() => stepPassage(1)}><ChevronDownIcon /></Button>
          </div> : null}
        </div> : null}
      </div>
      {error ? <Alert variant="destructive"><AlertDescription>{error}</AlertDescription><Button variant="outline" onClick={() => setReload((value) => value + 1)}>{t("sourceWorkspace.retrySource")}</Button></Alert> : null}
      {!detail && !error ? <p role="status" className="memory-loading-inline"><Spinner />{t("sourceWorkspace.openingSource")}</p> : null}
      {detail ? <article ref={readerRef} tabIndex={-1} aria-label={t("sourceWorkspace.sourceReader")} data-source-id={detail.sourceId}>
        <header className="knowledge-read-head source-read-head"><h2>{sourceTitle(detail)}</h2></header>
        <div className="memory-meta-row"><ConnectionOrigin origin={detail.origin} />{detail.sourceClass === "tool_result" && detail.originLabel?.startsWith("tool:") ? null : <Badge variant="outline">{metadataLabel(originLabel(), detail.sourceClass)}</Badge>}<span className="source-origin">{originalUrl ? <Button variant="link" title={originalUrl} onClick={() => void originalLink.open(originalUrl)}>
          {opensOriginalInPanel ? null : <ExternalLinkIcon aria-hidden="true" />}
          {t(detail.provided ? "sourceWorkspace.openStatedLink" : "sourceWorkspace.openOriginal", { hostname: new URL(originalUrl).hostname })}
        </Button> : compactOrigin(sourceOrigin(detail))}</span>{detail.publicationTime?.value ? <time title={detail.publicationTime.value}>{sourceDate(detail.publicationTime.value)}</time> : null}</div>
        {detail.provided ? <ProvidedNote provided={detail.provided} /> : null}
        <div className="source-evidence-summary"><span>{coverageLabel(detail.coverage)}</span>{detail.revisionId !== sourceRecord?.revisionId ? <span>{t("sourceWorkspace.savedVersion")}</span> : null}</div>
        {originalUrl && originalLink.failedUrl === originalUrl ? opensOriginalInPanel
          ? <p role="alert">{t("sourceWorkspace.couldNotOpenTheOriginalSource")}</p>
          : <ExternalLinkError url={originalUrl} /> : null}
        {span ? <section className="source-passage-section" aria-label={t(savedTable ? "sourceWorkspace.savedData" : "sourceWorkspace.savedPassage")}>
          <h3>{t(savedTable ? "sourceWorkspace.savedData" : "sourceWorkspace.savedPassage")}</h3>
          {savedTable && presentation ? <SavedSourceData key={span.spanId} table={savedTable} original={presentation.text} focus={focusTable} />
            : readingCitations && readableText ? passages.map((passage, index) => <Fragment key={passage.start}>
            {index > 0 && passages[index - 1]!.end < passage.start ? <div className="source-passage-elision" role="img" aria-label={t("sourceWorkspace.textBetweenCitedPassages")}>…</div> : null}
            <blockquote className="knowledge-passage" data-testid="knowledge-passage" data-cited-passage={index} {...quoteAttributes} data-quote-start-utf16={passage.start}>
              <SavedPassage text={readableText.slice(passage.start, passage.end)} marks={passage.marks} singleTableRow />
            </blockquote>
          </Fragment>) : textMedia(span.mediaType) && readableText ? <blockquote className="knowledge-passage" data-testid="knowledge-passage" {...quoteAttributes}>
            <SavedPassage text={readableText} />
          </blockquote> : <p>{t("sourceWorkspace.theOriginalFileIsSavedATextPreviewIsUnavailable")}</p>}
          {presentation && (presentation.contentOffset > 0 || presentation.nextOffset !== null) ? <p className="knowledge-hint">{t("sourceWorkspace.thisIsASavedSectionOfALongerDocument")}</p> : null}
          {presentation?.extractionTruncated ? <p className="knowledge-hint">{t("sourceWorkspace.textExtractionStoppedBeforeTheEndOfTheDocument")}</p> : null}
        </section> : <p>{t("sourceWorkspace.onlyTheSourceDetailsWereSavedNoPassageIsAvailable")}</p>}
        {span && !textMedia(span.mediaType) ? <div className="knowledge-inspector-actions"><Button variant="outline" disabled={exporting} onClick={() => void exportFile()}>{exporting ? t("sourceWorkspace.exporting") : detail.material === "original" ? t("sourceWorkspace.exportOriginalFile") : t("sourceWorkspace.exportSavedFile")}</Button>{exported ? <span role="status">{t("sourceWorkspace.fileExported")}</span> : null}{exportError ? <p role="alert">{exportError}</p> : null}</div> : null}
        {detail.spans.length > 1 ? <div className="knowledge-inspector-actions" aria-label={t("sourceWorkspace.sourcePassages")}>{detail.spans.map((item, index) => <Button key={item.spanId} variant={item.spanId === span?.spanId ? "secondary" : "ghost"} onClick={() => onOpen({ ...location, selection: { ...selected, kind: "source", sourceId: detail.sourceId, spanId: item.spanId } })}>{t("sourceWorkspace.passage", { index: formatNumber(index + 1) })}</Button>)}</div> : null}
        <div className="source-processing-status" data-attention={needsAttention(readerProcessing) || undefined}><div><span className="source-reader-eyebrow">{t("sourceWorkspace.worldProcessing")}</span><p role="status">{isMapProcessing(readerProcessing) ? <Spinner /> : null}{sourceProcessingLabel(readerProcessing)}</p>{detail.processing?.error ? <p className="knowledge-hint">{detail.processing.error}</p> : null}{processing === "paused_budget" ? <SourceRetry retry={retrySource(detail.recordId)} onRetried={readerRetried} /> : null}</div>{statusControl(readerProcessing)}</div>
        {detailRefreshError ? <div className="knowledge-inspector-actions"><p role="alert">{detailRefreshError}</p><Button variant="outline" disabled={detailRefreshing} onClick={() => void refreshDetail()}>{detailRefreshing ? t("sourceWorkspace.refreshingSource") : t("sourceWorkspace.refreshSourceDetails")}</Button></div> : null}
        {claimGroups.map(({ kind, label, claims }) => claims.length ? <details key={kind} className="source-related-claims" aria-label={label}>
          <summary>{label}<span>{formatNumber(claims.length)}</span></summary>
          <ul aria-label={label}>{claims.map((claim) => <li key={`${claim.worldRecordId}:${claim.worldRevisionId}:${claim.claimId}`}>
            <Button variant="ghost" onClick={() => onOpen({
              loupistId: location.loupistId, surface: "world", recordId: claim.worldRecordId, revisionId: claim.worldRevisionId,
              objectId: claim.claimId, selection: { kind: "claim", id: claim.claimId },
            })}>{claim.proposition}<small>{relationName(claim.relation, t)}</small></Button>
          </li>)}</ul>
        </details> : null)}
        {detail.derivation ? <p>{detail.derivation}</p> : null}
        <details className="knowledge-source-details"><summary>{t("sourceWorkspace.sourceDetails")}</summary><dl className="memory-meta-grid"><div><dt>{t("sourceWorkspace.coverage")}</dt><dd>{coverageLabel(detail.coverage)}</dd></div><div><dt>{t("sourceWorkspace.format")}</dt><dd>{metadataLabel(materialLabel(), detail.material)}</dd></div>{detail.acquiredAt ? <div><dt>{t("sourceWorkspace.saved")}</dt><dd>{timePointValue(detail.acquiredAt)}</dd></div> : null}</dl>{span?.contextNote ? <p>{span.contextNote}</p> : null}{span && spanSelectorLabel(span.selector) ? <p>{spanSelectorLabel(span.selector)}</p> : null}</details>
      </article> : null}
      {readingCitations && retainedSource && quoteCitedSelections ? <QuoteAsk scope="reader" within={readerRef} onQuote={addSelectionToQuestion} /> : null}
    </div> : null}
  </div>;
}

const MAX_SOURCE_BYTES = 8 * 1024 * 1024;
const bytesToBase64 = (bytes: Uint8Array) => {
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000) binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  return btoa(binary);
};
const readBytes = (file: File) => new Promise<Uint8Array>((resolve, reject) => {
  const reader = new FileReader();
  reader.onload = () => reader.result instanceof ArrayBuffer ? resolve(new Uint8Array(reader.result)) : reject(new Error(i18n.t("knowledge:sourceWorkspace.fileCouldNotBeRead")));
  reader.onerror = () => reject(reader.error);
  reader.readAsArrayBuffer(file);
});

type AddSourceProps = { bridge: LoupistBridge; loupistId: string; open: boolean; onOpenChange: (open: boolean) => void; onSaved: (location: KnowledgeLocation) => void };
export function AddSourceDialog({ bridge, loupistId, open, onOpenChange, onSaved }: AddSourceProps) {
  const { t } = useTranslation("knowledge");
  const [title, setTitle] = useState("");
  const [mode, setMode] = useState("text");
  const [origin, setOrigin] = useState("");
  const [text, setText] = useState("");
  const [file, setFile] = useState<KnowledgeSourceFile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [reading, setReading] = useState(false);
  const generation = useRef(0);
  const pending = useRef(false);
  useEffect(() => () => { generation.current++; }, []);
  const pickFile = async (picked: File) => {
    if (pending.current) return;
    const ticket = ++generation.current;
    setError(null);
    if (!picked.size || picked.size > MAX_SOURCE_BYTES) { setError(t("sourceWorkspace.chooseANonEmptyFileUpTo8MB")); setFile(null); setReading(false); return; }
    setReading(true);
    try {
      const bytes = await readBytes(picked);
      if (ticket !== generation.current) return;
      const mediaType = /^[\x21-\x7e]{3,128}$/.test(picked.type) && picked.type.includes("/") ? picked.type : "application/octet-stream";
      setFile({ fileName: picked.name, mediaType, bytesBase64: bytesToBase64(bytes) });
      setTitle((current) => current || picked.name);
    } catch (cause) { if (ticket === generation.current) setError(errorMessage(cause, t("sourceWorkspace.thisFileCouldNotBeRead"))); }
    finally { if (ticket === generation.current) setReading(false); }
  };
  const canSave = Boolean(title.trim() && (mode === "file" ? file : mode === "link" ? origin.trim() : text.trim()));
  const save = async () => {
    if (pending.current || reading || !canSave) return;
    const ticket = ++generation.current;
    pending.current = true; setSaving(true); setError(null);
    try {
      const retained = await bridge.knowledgeSourceRetain({ loupistId, title: title.trim(), originLocator: origin.trim() || (mode === "file" ? file?.fileName : "Pasted text") || "Pasted text", coverage: mode === "link" ? "metadata_only" : "included_full", text: mode === "text" ? text : "", file: mode === "file" ? file ?? undefined : undefined });
      if (ticket !== generation.current) return;
      setTitle(""); setOrigin(""); setText(""); setFile(null);
      onSaved({ loupistId, surface: "source", recordId: retained.recordId, revisionId: retained.revisionId, objectId: retained.sourceId, selection: { kind: "source", sourceId: retained.sourceId, spanId: retained.spanId ?? undefined } });
    } catch (cause) { if (ticket === generation.current) setError(errorMessage(cause, t("sourceWorkspace.couldNotSaveThisSource"))); }
    finally { if (ticket === generation.current) { pending.current = false; setSaving(false); } }
  };
  return <Dialog open={open} onOpenChange={(next) => { if (!saving && !reading) onOpenChange(next); }}>
    <DialogContent className="source-add-dialog" showCloseButton={!saving && !reading}>
      <DialogHeader><DialogTitle>{t("sourceWorkspace.addSource")}</DialogTitle><DialogDescription>{t("sourceWorkspace.keepMaterialForThisLoupistToUseInFutureResearch")}</DialogDescription></DialogHeader>
      <form className="dialog-form world-add-source" aria-label={t("sourceWorkspace.addSource")} onSubmit={(event) => { event.preventDefault(); void save(); }}>
        <DialogBody>
          {error ? <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert> : null}
          <label className="form-field">{t("sourceWorkspace.title")}<Input aria-label={t("sourceWorkspace.sourceTitle")} autoFocus value={title} disabled={saving} onChange={(event) => setTitle(event.target.value)} /></label>
          <Tabs value={mode} onValueChange={(next) => { setMode(next); setError(null); }}>
            <TabsList aria-label={t("sourceWorkspace.sourceMaterial")}>
              <TabsTrigger value="text" disabled={saving || reading}>{t("sourceWorkspace.text")}</TabsTrigger>
              <TabsTrigger value="file" disabled={saving || reading}>{t("sourceWorkspace.file")}</TabsTrigger>
              <TabsTrigger value="link" disabled={saving || reading}>{t("sourceWorkspace.link")}</TabsTrigger>
            </TabsList>
            <TabsContent value="text" className="source-add-section">
              <label className="form-field">{t("sourceWorkspace.originalText")}<Textarea aria-label={t("sourceWorkspace.sourceText")} rows={7} value={text} disabled={saving} onChange={(event) => setText(event.target.value)} /></label>
            </TabsContent>
            <TabsContent value="file" className="source-add-section">
              <label className="memory-dropzone" onDragOver={(event) => event.preventDefault()} onDrop={(event) => { event.preventDefault(); const picked = event.dataTransfer.files?.[0]; if (picked) void pickFile(picked); }}>
                <UploadIcon aria-hidden="true" /><span>{t("sourceWorkspace.chooseAFileOrDropItHere")}</span>
                <Input type="file" aria-label={t("sourceWorkspace.sourceFile")} disabled={saving || reading} onChange={(event) => { const picked = event.target.files?.[0]; event.target.value = ""; if (picked) void pickFile(picked); }} />
                <small>{t("sourceWorkspace.upTo8MBTheOriginalFileIsPreserved")}</small>
              </label>
              {reading ? <p role="status"><Spinner />{t("sourceWorkspace.readingFile")}</p> : null}
              {file ? <div className="knowledge-inspector-actions"><Badge variant="secondary"><FileTextIcon />{file.fileName}</Badge><Button type="button" variant="ghost" disabled={saving || reading} onClick={() => setFile(null)}>{t("sourceWorkspace.removeFile")}</Button></div> : null}
            </TabsContent>
            <TabsContent value="link" className="source-add-section">
              <label className="form-field">{t("sourceWorkspace.sourceOrURL")}<Input aria-label={t("sourceWorkspace.sourceOrigin")} placeholder={t("sourceWorkspace.articleFilingOrWebsite")} value={origin} disabled={saving} onChange={(event) => setOrigin(event.target.value)} /></label>
              <p className="field-hint">{t("sourceWorkspace.onlyTheLinkAndTitleAreSavedToKeepThe")}</p>
            </TabsContent>
          </Tabs>
          {mode !== "link" ? <Disclosure title={t("sourceWorkspace.sourceDetails")} description={t("sourceWorkspace.optionalOriginOrURL")}>
            <label className="form-field">{t("sourceWorkspace.sourceOrURL")}<Input aria-label={t("sourceWorkspace.sourceOrigin")} placeholder={t("sourceWorkspace.whereThisMaterialCameFrom")} value={origin} disabled={saving} onChange={(event) => setOrigin(event.target.value)} /></label>
          </Disclosure> : null}
        </DialogBody>
        <DialogFooter><Button type="button" variant="outline" disabled={saving || reading} onClick={() => onOpenChange(false)}>{t("sourceWorkspace.cancel")}</Button><Button type="submit" disabled={saving || reading || !canSave}>{saving ? t("sourceWorkspace.savingSource") : t("sourceWorkspace.saveSource")}</Button></DialogFooter>
      </form>
    </DialogContent>
  </Dialog>;
}
