import { useSettledScroll } from "./settledScroll";
import { ConnectionOrigin } from "../connections/custom/ConnectionOrigin";
import { formatDate, formatNumber } from "../../i18n/format";
import { dateValue, timePointValue, spanSelectorLabel } from "../../knowledge/presentation";
import i18n, { useTranslation } from "../../i18n";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { Disclosure } from "@/components/ui/disclosure";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ArrowLeftIcon, CheckIcon, ExternalLinkIcon, FileTextIcon, SearchIcon, Settings2Icon, UploadIcon, XIcon } from "lucide-react";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";
import { Badge } from "../ui/badge";
import { Alert, AlertDescription } from "../ui/alert";
import { EmptyState } from "../ui/empty-state";
import { Spinner } from "../ui/spinner";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import type { Coverage, LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin, KnowledgeSourceFile, RepresentationRole, SourceClass, SourceDetail } from "../../types";
import { errorMessage } from "../../lib/errors";
import { pinForSource } from "../../knowledge/location";
import { sourceOrigin, sourceTitle } from "../../knowledge/hostSource";
import { SegmentedControl, SegmentedControlItem } from "../ui/segmented-control";
import { relationName } from "../map/relations";

const originLabel = (): Record<SourceClass, string> => ({
  publisher_document: i18n.t("knowledge:sourceWorkspace.publisherDocument"), dataset: i18n.t("knowledge:sourceWorkspace.dataset"), user_statement: i18n.t("knowledge:sourceWorkspace.userStatement"),
  tool_result: i18n.t("knowledge:sourceWorkspace.toolResult"), analysis: i18n.t("knowledge:sourceWorkspace.analysis"), imported_contributor: i18n.t("knowledge:sourceWorkspace.importedSource"),
  code_revision: i18n.t("knowledge:sourceWorkspace.sourceCode"), web_page: i18n.t("knowledge:sourceWorkspace.webPage"), historical_document: i18n.t("knowledge:sourceWorkspace.historicalDocument"), user_file: i18n.t("knowledge:sourceWorkspace.userFile"),
});
const coverageLabel = (): Record<Coverage, string> => ({ included_full: i18n.t("knowledge:sourceWorkspace.fullDocument"), included_excerpt: i18n.t("knowledge:sourceWorkspace.excerpt"), upstream_truncated: i18n.t("knowledge:sourceWorkspace.incompleteSourceText"), metadata_only: i18n.t("knowledge:sourceWorkspace.sourceDetailsOnly"), omitted: i18n.t("knowledge:sourceWorkspace.contentNotSaved") });
const materialLabel = (): Record<RepresentationRole, string> => ({ original: i18n.t("knowledge:sourceWorkspace.originalMaterial"), provider_payload: i18n.t("knowledge:sourceWorkspace.downloadedCopy"), extracted_text: i18n.t("knowledge:sourceWorkspace.extractedText"), structured: i18n.t("knowledge:sourceWorkspace.structuredData"), excerpt: i18n.t("knowledge:sourceWorkspace.excerpt"), derived: i18n.t("knowledge:sourceWorkspace.processedMaterial") });
const compactOrigin = (value: string | undefined) => {
  if (!value) return "";
  if (value === "Pasted text") return i18n.t("knowledge:sourceWorkspace.pastedText");
  try { const url = new URL(value); if (["https:", "http:"].includes(url.protocol)) return url.hostname.replace(/^www\./, ""); } catch { /* Named or local source. */ }
  return value;
};
const sourceDate = (value: string | undefined) => {
  if (!value) return value;
  if (/^\d{4}(?:-\d{2})?$/.test(value)) return dateValue(value);
  // A publication day is a local calendar date, not a UTC instant.
  const date = new Date(/^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T00:00:00` : value);
  return Number.isNaN(date.getTime()) ? value : formatDate(date, { dateStyle: "medium" });
};
const textMedia = (value: string) => value.startsWith("text/") || value === "application/json";

const sourceProcessingLabel = (status?: string) => {
  switch (status) {
    case "queued": case "running": return i18n.t("knowledge:sourceWorkspace.worldIsUpdating");
    case "completed": return i18n.t("knowledge:sourceWorkspace.worldUpdated");
    case "paused_budget": case "paused": return i18n.t("knowledge:sourceWorkspace.worldProcessingPaused");
    case "failed_before_mutation": return i18n.t("knowledge:sourceWorkspace.worldProcessingFailed");
    case "reconcile_required": return i18n.t("knowledge:sourceWorkspace.worldUpdateNeedsReview");
    case "reconciled_without_retry": return i18n.t("knowledge:sourceWorkspace.sourceRetainedInterruptedProcessingWasNotRetried");
    case "cancelled_before_start": return i18n.t("knowledge:sourceWorkspace.worldProcessingCancelled");
    case "unsupported": case "no_text": return i18n.t("knowledge:sourceWorkspace.noTextForWorldProcessing");
    case "disabled": case "not_configured": return i18n.t("knowledge:sourceWorkspace.worldProcessingIsOff");
    default: return i18n.t("knowledge:sourceWorkspace.sourceSaved");
  }
};

type Props = { bridge: LoupistBridge; catalog: KnowledgeCatalog; location: KnowledgeLocation; query?: string; onQueryChange?: (query: string) => void; onManageProcessing?: () => void; onAddSource?: () => void; readerOnly?: boolean; compact?: boolean; onOpen: (location: KnowledgeLocation) => void; onPatch?: (location: KnowledgeLocation) => void; onAsk: (pin: KnowledgePin) => void; onChanged: () => void; onOpenLink?: (url: string) => void | Promise<void> };

export function SourceWorkspace({ bridge, catalog, location, query = "", onQueryChange, onManageProcessing, onAddSource, readerOnly = false, compact = false, onOpen, onPatch = onOpen, onAsk, onChanged, onOpenLink }: Props) {
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
  const generation = useRef(0);
  const readerRef = useRef<HTMLElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);
  const libraryRef = useRef<HTMLDivElement | null>(null);
  const scrollReaderRef = useRef<HTMLDivElement | null>(null);
  const filter = location.view?.sourceFilter ?? "all";
  const patchView = (view: KnowledgeLocation["view"]) => onPatch({ ...location, view: { ...location.view, ...view } });
  const changedRef = useRef(onChanged);
  changedRef.current = onChanged;
  const scopeKey = [location.loupistId, location.recordId, location.revisionId, selected?.sourceId, selected?.spanId, selected?.worldRecordId, selected?.worldRevisionId].join(":");
  const currentScope = useRef(scopeKey);
  currentScope.current = scopeKey;
  const reading = !compact && Boolean(selected?.sourceId || selected?.spanId);
  const onLibraryScroll = useSettledScroll((top) => patchView({ sourceScrollTop: top }), scopeKey);
  const onReaderScroll = useSettledScroll((top) => patchView({ readerScrollTop: top }), scopeKey);
  const readSelectedSource = useCallback(() => bridge.knowledgeSourceRead({
    loupistId: location.loupistId, recordId: location.recordId!, revisionId: location.revisionId!,
    sourceId: selected?.sourceId, spanId: selected?.spanId, worldRecordId: selected?.worldRecordId,
    worldRevisionId: selected?.worldRevisionId, purpose: "renderer_inspection",
  }), [bridge, location.loupistId, location.recordId, location.revisionId, selected?.sourceId, selected?.spanId, selected?.worldRecordId, selected?.worldRevisionId]);

  useEffect(() => {
    const ticket = ++generation.current;
    setDetail(null); setError(null); setProcessing(undefined); setDetailRefreshError(null); setDetailRefreshing(false); setExporting(false); setExported(false); setExportError(null);
    if (!reading || !location.recordId || !location.revisionId) return;
    void readSelectedSource()
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
      const next = await readSelectedSource();
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
    if (detail) { readerRef.current?.focus({ preventScroll: true }); if (scrollReaderRef.current) scrollReaderRef.current.scrollTop = location.view?.readerScrollTop ?? 0; }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- focuses and restores the reader once per opened source span.
  }, [detail?.sourceId, detail?.selectedSpanId]);

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

  const search = query.normalize("NFKC").trim().toLowerCase();
  const matching = catalog.sources.flatMap((record) => record.objects.map((item) => ({ record, item })))
    .filter(({ item }) => !search || `${sourceTitle(item)} ${item.summary ?? ""} ${sourceOrigin(item) ?? ""}`.normalize("NFKC").toLowerCase().includes(search));
  const isProcessing = (status?: string) => status === "queued" || status === "running";
  const needsAttention = (status?: string) => ["paused", "paused_budget", "failed_before_mutation", "reconcile_required", "cancelled_before_start", "unsupported", "no_text"].includes(status ?? "");
  const sources = matching.filter(({ item }) => filter === "all" || (filter === "processing" ? isProcessing(item.processing?.status) : needsAttention(item.processing?.status)));
  const originalUrl = (() => { try { const url = new URL(detail?.originLabel ?? ""); return url.protocol === "https:" || url.protocol === "http:" ? url.href : null; } catch { return null; } })();
  const span = detail?.spans.find((item) => item.spanId === detail.selectedSpanId) ?? detail?.spans[0];
  const readableText = span?.presentation?.text ?? span?.text;
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

  return <div className={`knowledge-source world-sources${reading ? " has-reader" : ""}${readerOnly ? " source-reader-only" : ""}${compact ? " is-library-only" : ""}`}>
    {!readerOnly ? <div ref={libraryRef} className="knowledge-browser source-library" onScroll={onLibraryScroll}>
      <div className="source-library-controls">
      {compact ? null : <div className="source-library-heading"><h2>{t("sourceWorkspace.savedSources")}</h2><span>{formatNumber(sources.length)}</span></div>}
      {onQueryChange ? <div className="memory-search source-search"><SearchIcon aria-hidden="true" /><Input aria-label={t("sourceWorkspace.searchSources")} placeholder={t("sourceWorkspace.companyFilingOrSource")} value={query} onChange={(event) => onQueryChange(event.target.value)} />{query ? <Button variant="ghost" size="icon-sm" aria-label={t("sourceWorkspace.clearSearch")} onClick={() => onQueryChange("")}><XIcon /></Button> : null}</div> : null}
      <SegmentedControl value={filter} onValueChange={(value) => patchView({ sourceFilter: value as "all" | "processing" | "attention", sourceScrollTop: 0 })} aria-label={t("sourceWorkspace.sourceProcessingFilters")}><SegmentedControlItem value="all">{t("sourceWorkspace.all")}</SegmentedControlItem><SegmentedControlItem value="processing">{matching.some(({ item }) => isProcessing(item.processing?.status)) ? t("sourceWorkspace.processingCount", { countText: formatNumber(matching.filter(({ item }) => isProcessing(item.processing?.status)).length) }) : t("sourceWorkspace.processing")}</SegmentedControlItem><SegmentedControlItem value="attention">{t("sourceWorkspace.needsAttention")}</SegmentedControlItem></SegmentedControl></div>
      {compact ? <div className="source-library-heading library-section-label"><h2>{t("sourceWorkspace.savedSources")}</h2><span>{formatNumber(sources.length)}</span>{onAddSource ? <Button variant="ghost" className="library-add" onClick={onAddSource}>{t("map.addInLibrary")}</Button> : null}</div> : null}
      {!sources.length ? <EmptyState title={search || filter !== "all" ? t("sourceWorkspace.noMatchingSources") : t("sourceWorkspace.noSourcesYet")} description={search || filter !== "all" ? t("sourceWorkspace.tryAnotherSearchOrChooseAll") : t("sourceWorkspace.addTextAFileOrALinkToUseIn")} /> : null}
      <ul ref={listRef} aria-label={t("sourceWorkspace.sources")}>
        {sources.map(({ record, item }) => <li key={`${record.recordId}:${item.id}`}>
          <Button variant="ghost" className="source-library-row" data-testid="source-library-row" data-source-id={item.id} aria-current={item.id === selected?.sourceId} onClick={() => onOpen({ loupistId: location.loupistId, surface: "source", recordId: record.recordId, revisionId: record.revisionId, objectId: item.id, view: { ...location.view, readerScrollTop: 0, focusSourceId: item.id }, selection: { kind: "source", sourceId: item.id } })}>
            <FileTextIcon className="memory-rail-icon" aria-hidden="true" />
            <span className="memory-rail-copy"><strong>{sourceTitle(item)}</strong>{item.summary ? <span className="source-summary">{item.summary}</span> : null}
              <small className="source-row-meta"><ConnectionOrigin origin={item.origin} /><span className="source-row-origin" title={sourceOrigin(item)}>{compactOrigin(sourceOrigin(item))}</span>{item.publicationTime?.value ?? item.acquiredAt?.value ? <time title={item.publicationTime?.value ?? item.acquiredAt?.value}>{sourceDate(item.publicationTime?.value ?? item.acquiredAt?.value)}</time> : null}
              <span className="source-row-status" data-attention={needsAttention(item.processing?.status) || undefined}>{isProcessing(item.processing?.status) ? <Spinner /> : item.processing?.status === "completed" ? <CheckIcon aria-hidden="true" /> : null}{sourceProcessingLabel(item.processing?.status)}</span></small>
            </span>
          </Button>
        </li>)}
      </ul>
    </div> : null}
    {reading ? <div ref={scrollReaderRef} className="knowledge-reading source-reader" onScroll={onReaderScroll}>
      <div className="memory-reader-toolbar">
        {!readerOnly ? <Button variant="ghost" size="sm" aria-label={t("sourceWorkspace.closeSource")} onClick={closeReader}><ArrowLeftIcon />{t("sourceWorkspace.backToSources")}</Button> : <span>{t("sourceWorkspace.savedSourceReadOnly")}</span>}
        {detail ? <Button size="sm" onClick={() => onAsk(pinForSource(detail, location.loupistId))}>{t("sourceWorkspace.addToQuestion")}</Button> : null}
      </div>
      {error ? <Alert variant="destructive"><AlertDescription>{error}</AlertDescription><Button variant="outline" onClick={() => setReload((value) => value + 1)}>{t("sourceWorkspace.retrySource")}</Button></Alert> : null}
      {!detail && !error ? <p role="status" className="memory-loading-inline"><Spinner />{t("sourceWorkspace.openingSource")}</p> : null}
      {detail ? <article ref={readerRef} tabIndex={-1} aria-label={t("sourceWorkspace.sourceReader")} data-source-id={detail.sourceId}>
        <header className="knowledge-read-head source-read-head"><h2>{sourceTitle(detail)}</h2></header>
        <div className="memory-meta-row"><ConnectionOrigin origin={detail.origin} /><Badge variant="outline">{originLabel()[detail.sourceClass]}</Badge><span className="source-origin">{originalUrl ? <Button variant="link" title={originalUrl} onClick={() => { void Promise.resolve().then(() => (onOpenLink ?? bridge.openExternalUrl.bind(bridge))(originalUrl)).catch((cause: unknown) => setExportError(errorMessage(cause, t("sourceWorkspace.couldNotOpenTheOriginalSource")))); }}><ExternalLinkIcon />{t("sourceWorkspace.openOriginal", { hostname: new URL(originalUrl).hostname })}</Button> : compactOrigin(sourceOrigin(detail))}</span>{detail.publicationTime?.value ? <time title={detail.publicationTime.value}>{sourceDate(detail.publicationTime.value)}</time> : null}</div>
        <div className="source-evidence-summary"><span>{coverageLabel()[detail.coverage]}</span>{detail.revisionId !== sourceRecord?.revisionId ? <span>{t("sourceWorkspace.savedVersion")}</span> : null}</div>
        {exportError && originalUrl ? <p role="alert">{exportError}</p> : null}
        {span ? <section className="source-passage-section" aria-label={t("sourceWorkspace.savedPassage")}><h3>{t("sourceWorkspace.savedPassage")}</h3>{textMedia(span.mediaType) && readableText ? <blockquote className="knowledge-passage" data-testid="knowledge-passage" data-quote-source={readerOnly ? `source:${detail.sourceId}:${span.spanId}` : undefined} data-quote-retained-source={readerOnly ? JSON.stringify({ loupistId: location.loupistId, recordId: detail.recordId, revisionId: detail.revisionId, sourceId: detail.sourceId, spanId: span.spanId, worldRecordId: selected?.worldRecordId, worldRevisionId: selected?.worldRevisionId, purpose: "renderer_inspection" }) : undefined}>{readableText}</blockquote> : <p>{t("sourceWorkspace.theOriginalFileIsSavedATextPreviewIsUnavailable")}</p>}{span.presentation && (span.presentation.contentOffset > 0 || span.presentation.nextOffset !== null) ? <p className="knowledge-hint">{t("sourceWorkspace.thisIsASavedSectionOfALongerDocument")}</p> : null}{span.presentation?.extractionTruncated ? <p className="knowledge-hint">{t("sourceWorkspace.textExtractionStoppedBeforeTheEndOfTheDocument")}</p> : null}</section> : <p>{t("sourceWorkspace.onlyTheSourceDetailsWereSavedNoPassageIsAvailable")}</p>}
        {span && !textMedia(span.mediaType) ? <div className="knowledge-inspector-actions"><Button variant="outline" disabled={exporting} onClick={() => void exportFile()}>{exporting ? t("sourceWorkspace.exporting") : detail.material === "original" ? t("sourceWorkspace.exportOriginalFile") : t("sourceWorkspace.exportSavedFile")}</Button>{exported ? <span role="status">{t("sourceWorkspace.fileExported")}</span> : null}{exportError ? <p role="alert">{exportError}</p> : null}</div> : null}
        {detail.spans.length > 1 ? <div className="knowledge-inspector-actions" aria-label={t("sourceWorkspace.sourcePassages")}>{detail.spans.map((item, index) => <Button key={item.spanId} variant={item.spanId === span?.spanId ? "secondary" : "ghost"} onClick={() => onOpen({ ...location, selection: { ...selected, kind: "source", sourceId: detail.sourceId, spanId: item.spanId } })}>{t("sourceWorkspace.passage", { index: formatNumber(index + 1) })}</Button>)}</div> : null}
        <div className="source-processing-status" data-attention={needsAttention(processing) || undefined}><div><span className="source-reader-eyebrow">{t("sourceWorkspace.worldProcessing")}</span><p role="status">{isProcessing(processing) ? <Spinner /> : null}{sourceProcessingLabel(processing)}</p>{detail.processing?.error ? <p className="knowledge-hint">{detail.processing.error}</p> : null}</div>{onManageProcessing ? <Button variant="ghost" size="icon-sm" aria-label={t("sourceWorkspace.worldProcessingSettings")} title={t("sourceWorkspace.worldProcessingSettings")} onClick={onManageProcessing}><Settings2Icon /></Button> : null}</div>
        {detailRefreshError ? <div className="knowledge-inspector-actions"><p role="alert">{detailRefreshError}</p><Button variant="outline" disabled={detailRefreshing} onClick={() => void refreshDetail()}>{detailRefreshing ? t("sourceWorkspace.refreshingSource") : t("sourceWorkspace.refreshSourceDetails")}</Button></div> : null}
        {detail.relatedClaims.length ? <details className="source-related-claims" aria-label={t("sourceWorkspace.relatedRelationships")}><summary>{t("sourceWorkspace.relationshipsUsingThisSource")}<span>{formatNumber(detail.relatedClaims.length)}</span></summary><ul aria-label={t("sourceWorkspace.relatedRelationships")}>{detail.relatedClaims.map((claim) => <li key={`${claim.worldRecordId}:${claim.worldRevisionId}:${claim.claimId}`}><Button variant="ghost" onClick={() => onOpen({ loupistId: location.loupistId, surface: "world", recordId: claim.worldRecordId, revisionId: claim.worldRevisionId, objectId: claim.claimId, selection: { kind: "claim", id: claim.claimId } })}>{claim.proposition}<small>{relationName(claim.relation, t)}</small></Button></li>)}</ul></details> : null}
        {detail.derivation ? <p>{detail.derivation}</p> : null}
        <details className="knowledge-source-details"><summary>{t("sourceWorkspace.sourceDetails")}</summary><dl className="memory-meta-grid"><div><dt>{t("sourceWorkspace.coverage")}</dt><dd>{coverageLabel()[detail.coverage]}</dd></div><div><dt>{t("sourceWorkspace.format")}</dt><dd>{materialLabel()[detail.material]}</dd></div>{detail.acquiredAt ? <div><dt>{t("sourceWorkspace.saved")}</dt><dd>{timePointValue(detail.acquiredAt)}</dd></div> : null}</dl>{span?.contextNote ? <p>{span.contextNote}</p> : null}{span && spanSelectorLabel(span.selector) ? <p>{spanSelectorLabel(span.selector)}</p> : null}</details>
      </article> : null}
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
