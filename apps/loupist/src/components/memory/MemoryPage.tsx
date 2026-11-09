import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatDateTime, formatNumber, formatTime } from "@/i18n/format";
import { useCallback, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { ArrowLeftIcon, SearchIcon, XIcon } from "lucide-react";
import type { ChatThread, LoupistSummary, LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin, LensKind } from "../../types";
import type { GrowthEvent, KnowledgeMapNode, LearningReview, MemoryUsage, Principle } from "../../contracts/knowledge";
import type { PageView } from "../../contracts/lens";
import type { HoldingsSummary, WatchlistEntry } from "../../contracts/userContext";
import { useConfiguration } from "../../app/useConfiguration";
import { errorMessage } from "../../lib/errors";
import { revealFocused } from "../../lib/revealFocused";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { displayPageTitle, displaySectionHeading } from "../../knowledge/hostDefaults";
import { chatTitle } from "../../app/chatTitle";
import { LensLearningPanel } from "../knowledge/LensLearningPanel";
import { PageReader } from "./PageReader";
import { PrincipleSource } from "./PrincipleSource";
import { entityLabel } from "../map/labels";
import { useAssetPageOpener } from "../assets/assetOpener";
import type { PackImportView, PackSuggestionGroup } from "../../contracts/sharing";
import { sharingReleaseVisible } from "../../sharing/releaseVisibility";
import { ShareSheet } from "../sharing/ShareSheet";
import { PackSuggestions } from "../sharing/PackSuggestions";
import { SHARED_SECTION_ITEM, SharedPacks } from "../sharing/SharedPacks";
import { SharedOrigin } from "../sharing/SharedOrigin";
import { HoldingsSheet } from "../holdings/HoldingsSheet";
import { SourceWorkspace } from "../knowledge/SourceWorkspace";
import "@/styles/memory-page.css";

const t = i18n.getFixedT(null, "knowledge");
export type MemorySegment = "about" | "pages" | "history";
const SEGMENTS: readonly MemorySegment[] = ["about", "pages", "history"];
const KIND_ORDER: readonly LensKind[] = ["subject", "idea", "method", "principle", "perspective"];
const day = (ms: number) => formatDate(ms, { year: undefined, month: "short", day: "numeric" });

type Item = {
  kind: LensKind;
  id: string;
  title: string;
  summary: string;
  updatedAtMs?: number;
  paused?: boolean;
  learned?: boolean;
  principle?: Principle;
};

type Data = {
  paused: Set<string>;
  principles: Principle[];
  catalog: KnowledgeCatalog;
  userRows: HistoryRow[];
  usage: Map<string, MemoryUsage>;
  reviews: LearningReview[];
  events: GrowthEvent[];
  learnMethods: boolean | null;
  watchlist: WatchlistEntry[];
  /** Watched and held Map names, for showing a followed asset by its local name. */
  followedNodes: KnowledgeMapNode[];
  holdings: HoldingsSummary | null;
  /** Shared Loupists added here and their pending suggestions (W19.4). */
  suggestions: PackSuggestionGroup[];
  imports: PackImportView[];
  indexConfigured: boolean;
};

type Props = {
  bridge: LoupistBridge;
  loupist: LoupistSummary;
  threads: readonly ChatThread[];
  /** A record to open in the reader, e.g. from a receipt segment. */
  initialItem?: string;
  onOpenChat: (threadId: string) => void;
  onOpenMap: () => void;
  onChanged?: () => void;
  onOpenLink: (url: string) => void | Promise<void>;
  /** Opens a note, Source or World target linked from a method's evidence. */
  onOpenKnowledge: (location: KnowledgeLocation) => void;
  onAsk: (pin: KnowledgePin) => void;
};

/**
 * "What {Loupist} knows": what the Loupist knows about the user, the principles the
 * user taught, the methods it learned, and when. Every write is a typed intent
 * that Rust validates; nothing here shows holdings quantities or values.
 */
export function MemoryPage({ bridge, loupist, threads, initialItem, onOpenChat, onOpenMap, onChanged, onOpenLink, onOpenKnowledge, onAsk }: Props) {
  useTranslation(["knowledge", "onboarding"]);
  const loupistId = loupist.id;
  const [refresh, setRefresh] = useState(0);
  const [data, setData] = useState<Data | null>(null);
  const [error, setError] = useState(false);
  const [segment, setSegment] = useState<MemorySegment | null>(null);
  const [selectedId, setSelectedId] = useState<string | undefined>(initialItem);
  // The first page shows on its own when Lens opens; only a page someone opened takes focus.
  const [chosen, setChosen] = useState(Boolean(initialItem));
  const [query, setQuery] = useState("");
  const [compactReader, setCompactReader] = useState(Boolean(initialItem));
  const [historyKey, setHistoryKey] = useState<string | undefined>();
  const config = useConfiguration(bridge, { kind: "project", loupistId });
  const [sourceLocation, setSourceLocation] = useState<KnowledgeLocation | null>(null);
  const [share, setShare] = useState<{ recordId?: string } | null>(null);
  const sharedRef = useRef<HTMLElement>(null);
  const showSharing = sharingReleaseVisible();
  const changed = useCallback(() => { setRefresh(value => value + 1); onChanged?.(); }, [onChanged]);

  useEffect(() => {
    let live = true;
    setError(false);
    void Promise.all([
      bridge.principleList({ loupistId }),
      bridge.knowledgeCatalog({ loupistId }),
      bridge.knowledgeMemoryUsage({ loupistId }).catch(() => []),
      bridge.learningReviewList({ loupistId, limit: 50 }).catch(() => []),
      bridge.knowledgeGrowthEvents({ loupistId, limit: 500 }).then(page => page.events).catch(() => []),
      bridge.memoryLearningSettingsGet({ loupistId }).then(settings => settings.learnMethods).catch(() => null),
      bridge.watchlistList().catch(() => []),
      bridge.holdingsSummary().catch(() => null),
      bridge.knowledgeMap({ loupistId, maxNodes: 1000 }).then(map => map.nodes.filter(node => node.kind === "entity" && (node.watched || node.held))).catch(() => []),
      bridge.packSuggestions({ loupistId }).catch(() => []),
      bridge.packList({ loupistId }).catch(() => []),
      bridge.githubAccount().then(account => account.indexConfigured).catch(() => false),
      bridge.knowledgeMemoryControls({ loupistId }),
    ]).then(async ([principles, catalog, usage, reviews, events, learnMethods, watchlist, holdings, followedNodes, suggestions, imports, indexConfigured, controls]) => {
      const touched = [...new Set(events.filter(event => event.kind === "lens_revised" && event.actor === "user" && event.recordId).map(event => event.recordId!))].slice(0, 20);
      const pages = await Promise.all(touched.map(recordId => bridge.knowledgeLensRead({ loupistId, recordId }).catch(() => null)));
      if (!live) return;
      setData({
        paused: new Set(controls.paused),
        principles: principles.filter(item => !item.archived),
        catalog,
        userRows: pages.flatMap(page => page ? sectionRows(page) : []),
        usage: new Map(usage.map(item => [item.recordId, item])),
        reviews, events, learnMethods, watchlist, holdings, followedNodes, suggestions, imports, indexConfigured,
      });
    }).catch(() => { if (live) setError(true); });
    return () => { live = false; };
  }, [bridge, loupistId, refresh]);

  useEffect(() => bridge.onLearningReviewFinished(event => { if (event.loupistId === loupistId) setRefresh(value => value + 1); }), [bridge, loupistId]);

  const items = useMemo<Item[]>(() => !data ? [] : [
    ...data.principles.map(principle => ({ kind: "principle" as const, id: principle.recordId, title: principle.statement, summary: principle.why ?? "", updatedAtMs: principle.createdAtMs, paused: data.paused.has(principle.recordId), principle })),
    ...data.catalog.lenses.filter(lens => lens.lensKind && lens.lensKind !== "principle" && !lens.archived).map(lens => ({
      kind: lens.lensKind as LensKind, id: lens.recordId, summary: lens.summary ?? "", updatedAtMs: lens.updatedAtMs, paused: data.paused.has(lens.recordId), learned: lens.learned,
      title: displayPageTitle(lens.lensKind as LensKind, lens.title),
    })),
  ], [data]);
  useEffect(() => {
    if (!data || segment) return;
    const target = items.find(item => item.id === initialItem);
    const pendingSuggestions = showSharing && data.suggestions.some(group => group.items.some(item => item.state === "pending"));
    const sharedLanding = showSharing && initialItem === SHARED_SECTION_ITEM;
    if (!initialItem) setSelectedId(KIND_ORDER.flatMap(kind => items.filter(item => item.kind === kind))[0]?.id);
    setSegment(sharedLanding ? "about" : target || pendingSuggestions || items.length ? "pages" : "about");
  }, [data, items, initialItem, segment, showSharing]);
  // "About this shared Loupist" lands on the shared Loupists section.
  useEffect(() => {
    if (showSharing && initialItem === SHARED_SECTION_ITEM && segment === "about") sharedRef.current?.scrollIntoView?.({ block: "start" });
  }, [showSharing, initialItem, segment, data]);
  const suggestions = showSharing && data ? <PackSuggestions bridge={bridge} loupistId={loupistId} groups={data.suggestions}
    onChanged={groups => { setData(current => current && { ...current, suggestions: groups }); changed(); }} /> : null;

  const terms = query.trim().toLowerCase();
  const [hits, setHits] = useState<ReadonlySet<string> | null>(null);
  useEffect(() => {
    if (!terms) { setHits(null); return; }
    let live = true;
    void bridge.knowledgeLensSearch({ loupistId, query: query.trim() }).then(found => { if (live) setHits(new Set(found.map(hit => hit.recordId))); }).catch(() => { if (live) setHits(new Set()); });
    return () => { live = false; };
  }, [bridge, loupistId, query]);
  const shown = items.filter(item => !terms || (item.kind === "principle" ? `${item.title} ${item.summary}`.toLowerCase().includes(terms) : hits?.has(item.id)));
  const selected = items.find(item => item.id === selectedId);
  const remember = config.snapshot ? config.snapshot.effective.chat.use_memory && config.snapshot.effective.chat.update_memory : null;
  const paused = remember === false && data?.learnMethods === false;
  const resume = async () => {
    await Promise.all([
      config.update({ chat: { use_memory: true, update_memory: true } }, []),
      bridge.memoryLearningSettingsSet({ loupistId, learnMethods: true }),
    ]);
    changed();
  };

  const open = (id: string) => { setSelectedId(id); setChosen(true); setCompactReader(true); };
  const openKnowledge = (location: KnowledgeLocation) => {
    if (location.surface === "source") { setSourceLocation(location); return; }
    if (location.surface === "lens" && items.some(item => item.id === location.recordId)) {
      const target = items.find(item => item.id === location.recordId)!;
      setSegment("pages"); open(target.id); setSourceLocation(null); return;
    }
    onOpenKnowledge(location);
  };
  const current = segment ?? "pages";
  const title = t("memoryPage.title", { name: loupist.name });
  if (sourceLocation && data) return <section className="memory-page memory-source-page" aria-label={title}>
    <header className="memory-page-header"><Button variant="ghost" size="sm" onClick={() => setSourceLocation(null)}><ArrowLeftIcon aria-hidden="true" />{t("memoryPage.backToMethod")}</Button><h1>{title}</h1></header>
    <SourceWorkspace bridge={bridge} catalog={data.catalog} location={sourceLocation} readerOnly onOpen={openKnowledge} onPatch={setSourceLocation} onAsk={onAsk} onChanged={changed} onOpenLink={onOpenLink} />
  </section>;
  return <section className="memory-page" aria-label={title} data-segment={current} data-reader-open={compactReader && (current === "history" || Boolean(selected) && current === "pages") || undefined}>
    <header className="memory-page-header">
      <h1>{title}</h1>
      {showSharing ? <Button variant="outline" size="sm" data-share-loupist onClick={() => setShare({})}>{i18n.t("sharing:share")}</Button> : null}
    </header>
    {showSharing ? <ShareSheet bridge={bridge} loupistId={loupistId} loupistName={loupist.name} scope={share?.recordId ? { kind: "item", recordId: share.recordId } : { kind: "loupist" }} open={share !== null} onOpenChange={open => { if (!open) setShare(null); }} /> : null}
    <div className="memory-page-subbar">
      <SegmentTabs value={current} counts={{ pages: data ? items.length : undefined }} onChange={value => { setSegment(value); setCompactReader(false); }} />
      {current === "pages" ? <label className="memory-page-search">
        <SearchIcon aria-hidden="true" />
        <Input type="search" aria-label={t("memoryPage.search")} placeholder={t("memoryPage.search")} value={query} onChange={event => setQuery(event.target.value)} />
        {query ? <Button variant="ghost" size="icon-sm" aria-label={t("memoryPage.clearSearch")} onClick={() => setQuery("")}><XIcon /></Button> : null}
      </label> : null}
    </div>
    {paused ? <div className="memory-page-paused" role="status"><span>{t("memoryPage.paused.banner")}</span><Button size="sm" variant="outline" onClick={() => void resume()}>{t("memoryPage.actions.resume")}</Button></div> : null}
    {error ? <div className="memory-page-error" role="alert"><span>{t("memoryPage.error")}</span><Button size="sm" variant="outline" onClick={() => setRefresh(value => value + 1)}>{t("memoryPage.retry")}</Button></div> : null}
    {!data && !error ? <div className="memory-page-loading" role="status" aria-label={t("memoryPage.loading")}><span /><span /><span /></div> : null}
    {data ? <div className="memory-page-body" data-segment={current} onFocus={revealFocused}>
      {current === "about" ? <AboutYou bridge={bridge} loupist={loupist} data={data} onChanged={changed}
        shared={showSharing ? <SharedPacks ref={sharedRef} bridge={bridge} loupistId={loupistId} imports={data.imports} indexConfigured={data.indexConfigured}
          onChanged={imports => { setData(current => current && { ...current, imports }); changed(); }} /> : null} /> : null}
      {current === "pages" ? <>
        <ItemList label={t("memoryPage.tabs.pages")} description={t("memoryPage.pages.description")} items={shown} selectedId={selected?.id} usage={data.usage} onOpen={open}
          before={suggestions} searching={Boolean(terms)} empty={<p className="memory-empty">{t("memoryPage.pages.empty")}</p>} />
        <Reader item={selected} takeFocus={chosen} bridge={bridge} loupistId={loupistId} usage={data.usage} paused={Boolean(selected && data.paused.has(selected.id))} threads={threads} onOpenChat={onOpenChat} onChanged={changed} onClose={() => setCompactReader(false)} backLabel={t("memoryPage.tabs.pages")} catalog={data.catalog} onOpenKnowledge={openKnowledge} onOpenPage={open} onAsk={onAsk}
          onShare={showSharing ? recordId => setShare({ recordId }) : undefined} onAboutShared={showSharing ? () => { setSegment("about"); setCompactReader(false); requestAnimationFrame(() => sharedRef.current?.scrollIntoView?.({ block: "start" })); } : undefined} />
      </> : null}
      {current === "history" ? (() => {
        // The newest row that names a principle or method opens by default, so the tab shows how something was learned.
        const rows = historyRows(data, threads);
        const row = rows.find(entry => entry.key === historyKey) ?? rows.find(entry => entry.itemId && items.some(item => item.id === entry.itemId)) ?? rows[0];
        const item = row?.itemId ? items.find(entry => entry.id === row.itemId) : undefined;
        return <>
          <History rows={rows} selectedKey={row?.key} onSelect={entry => { setHistoryKey(entry.key); setCompactReader(true); }} />
          {item ? <Reader item={item} bridge={bridge} loupistId={loupistId} usage={data.usage} paused={data.paused.has(item.id)} threads={threads} onOpenChat={onOpenChat} onChanged={changed} onClose={() => setCompactReader(false)} backLabel={t("memoryPage.tabs.history")} catalog={data.catalog} onOpenKnowledge={openKnowledge} onOpenPage={id => { setSegment("pages"); open(id); }} onAsk={onAsk} />
            : row ? <HistoryNote row={row} onOpenMap={onOpenMap} onClose={() => setCompactReader(false)} /> : null}
        </>;
      })() : null}
    </div> : null}
  </section>;
}

function SegmentTabs({ value, counts, onChange }: { value: MemorySegment; counts: Partial<Record<MemorySegment, number>>; onChange: (value: MemorySegment) => void }) {
  const refs = useRef(new Map<MemorySegment, HTMLButtonElement>());
  const keyDown = (event: KeyboardEvent, index: number) => {
    const next = event.key === "ArrowRight" ? (index + 1) % SEGMENTS.length : event.key === "ArrowLeft" ? (index + SEGMENTS.length - 1) % SEGMENTS.length : event.key === "Home" ? 0 : event.key === "End" ? SEGMENTS.length - 1 : -1;
    if (next < 0) return;
    event.preventDefault();
    onChange(SEGMENTS[next]);
    refs.current.get(SEGMENTS[next])?.focus();
  };
  return <div className="memory-page-tabs" role="tablist" aria-label={t("memoryPage.sections")}>
    {SEGMENTS.map((segment, index) => <button key={segment} ref={element => { if (element) refs.current.set(segment, element); }} type="button" role="tab" aria-selected={segment === value} tabIndex={segment === value ? 0 : -1} onClick={() => onChange(segment)} onKeyDown={event => keyDown(event, index)}>
      <span className="memory-tab-full">{t(`memoryPage.tabs.${segment}`)}</span>
      <span className="memory-tab-short" aria-hidden="true">{t(`memoryPage.tabsShort.${segment}`)}</span>
      {counts[segment] ? <> <span className="memory-tab-count">{formatNumber(counts[segment]!)}</span></> : null}
    </button>)}
  </div>;
}

const usageLine = (item: Item, usage: Map<string, MemoryUsage>) => {
  const used = usage.get(item.id);
  if (item.kind === "principle") return used?.usedCount ? t("memoryPage.row.used", { count: used.usedCount, formattedCount: formatNumber(used.usedCount), date: day(used.lastUsedAtMs) }) : t("memoryPage.row.taught", { date: day(item.updatedAtMs ?? 0) });
  if (item.kind === "perspective") return t("memoryPage.perspectiveDescription");
  const learned = item.updatedAtMs ? day(item.updatedAtMs) : "";
  return used?.usedCount ? t("memoryPage.row.learned", { date: learned, count: used.usedCount, formattedCount: formatNumber(used.usedCount) }) : learned ? t("memoryPage.row.learnedOnly", { date: learned }) : t("memoryPage.row.neverUsed");
};
const kindLabel = (kind: LensKind) => t(`pageReader.kind.${kind}`);

function ItemList({ label, description, items, selectedId, usage, onOpen, before, empty, searching }: { label: string; description: string; items: Item[]; selectedId?: string; usage: Map<string, MemoryUsage>; onOpen: (id: string) => void; before?: ReactNode; empty: ReactNode; searching: boolean }) {
  const listId = useId();
  const refs = useRef(new Map<string, HTMLButtonElement>());
  const groups = KIND_ORDER.map(kind => ({ kind, items: items.filter(item => item.kind === kind) })).filter(group => group.items.length);
  const flat = groups.flatMap(group => group.items);
  const move = (event: KeyboardEvent, index: number) => {
    const next = event.key === "ArrowDown" ? index + 1 : event.key === "ArrowUp" ? index - 1 : -1;
    if (next < 0 || next >= flat.length) return;
    event.preventDefault();
    refs.current.get(flat[next].id)?.focus();
  };
  return <div className="memory-page-list">
    <h2 id={listId} className="memory-section-label">{label}</h2>
    <p className="memory-section-description">{description}</p>
    {before}
    {groups.length ? groups.map(group => <section key={group.kind} className="memory-page-group" aria-label={t(`memoryPage.groups.${group.kind}`)}>
      <h3 className="memory-group-label"><span>{t(`memoryPage.groups.${group.kind}`)}</span><span className="memory-group-count" aria-hidden="true">{formatNumber(group.items.length)}</span><span className="sr-only">{t("memoryPage.groupCount", { count: group.items.length, formattedCount: formatNumber(group.items.length) })}</span></h3>
      <ul>
        {group.items.map(item => <li key={item.id}>
          <button ref={element => { if (element) refs.current.set(item.id, element); else refs.current.delete(item.id); }} type="button" className="memory-row" aria-current={item.id === selectedId || undefined}
            aria-label={`${item.title}, ${kindLabel(item.kind)}, ${usageLine(item, usage)}`} onClick={() => onOpen(item.id)} onKeyDown={event => move(event, flat.indexOf(item))}>
            <i className={`memory-marker is-${item.kind}`} aria-hidden="true" />
            <span><strong>{item.title}</strong><small>{item.paused ? t("memoryPage.itemPaused") : item.summary && item.kind !== "principle" ? item.summary : usageLine(item, usage)}</small></span>
          </button>
        </li>)}
      </ul>
    </section>) : searching ? <p className="memory-empty">{t("memoryPage.pages.noMatch")}</p> : empty}
  </div>;
}

function UsedIn({ usage, threads, onOpenChat }: { usage?: MemoryUsage; threads: readonly ChatThread[]; onOpenChat: (threadId: string) => void }) {
  const rows = (usage?.usedIn ?? []).flatMap(entry => {
    const thread = threads.find(item => item.id === entry.chatId);
    return thread ? [{ ...entry, title: thread.home || thread.title.trim() ? chatTitle(thread) : t("memoryPage.reader.untitledChat") }] : [];
  });
  if (!rows.length) return null;
  return <section className="memory-reader-section"><h3>{t("memoryPage.reader.usedIn")}</h3><p className="memory-meta">{t("memoryPage.reader.usageDefinition")}</p><ul>
    {rows.map(row => <li key={row.chatId}><button type="button" onClick={() => onOpenChat(row.chatId)}>{row.title}</button><small>{day(row.atMs)}</small></li>)}
  </ul></section>;
}

function Reader({ item, takeFocus, bridge, loupistId, usage, paused, threads, onOpenChat, onChanged, onClose, backLabel, catalog, onOpenKnowledge, onOpenPage, onAsk, onShare, onAboutShared }: {
  item?: Item; takeFocus?: boolean; bridge: LoupistBridge; loupistId: string; usage: Map<string, MemoryUsage>; paused: boolean; threads: readonly ChatThread[];
  onOpenChat: (threadId: string) => void; onChanged: () => void; onClose: () => void; backLabel: string;
  catalog: KnowledgeCatalog; onOpenKnowledge: (location: KnowledgeLocation) => void; onOpenPage: (recordId: string) => void; onAsk: (pin: KnowledgePin) => void;
  /** "Share this method" for a principle (W19.2). */
  onShare?: (recordId: string) => void; onAboutShared?: () => void;
}) {

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { setError(null); }, [item?.id]);
  if (!item) return <aside className="memory-reader is-empty" aria-hidden="true" />;
  const act = async (action: () => Promise<unknown>) => {
    setBusy(true); setError(null);
    try { await action(); onChanged(); }
    catch (cause) { setError(errorMessage(cause, t("memoryPage.actionError"))); }
    finally { setBusy(false); }
  };
  const pauseAction = <Button size="sm" variant="ghost" disabled={busy} onClick={() => void act(() => bridge.knowledgeLensPause({ loupistId, recordId: item.id, paused: !paused }))}>{t(paused ? "memoryPage.actions.resume" : "memoryPage.actions.pause")}</Button>;
  const back = <Button variant="ghost" size="sm" className="memory-reader-back" onClick={onClose}><ArrowLeftIcon aria-hidden="true" /><span className="memory-reader-back-label">{backLabel}</span></Button>;
  const kind = <p className="memory-reader-kind">{kindLabel(item.kind)} · {usageLine(item, usage)}</p>;
  const pausedHint = paused ? <p className="memory-meta" role="status">{t("memoryPage.itemPausedHint")}</p> : null;
  if (item.kind !== "principle" || !item.principle) {
    return <aside className="memory-reader is-lens" aria-label={item.title}>
      {pausedHint}
      {error ? <p className="memory-reader-error" role="alert">{error}</p> : null}
      <PageReader bridge={bridge} loupistId={loupistId} recordId={item.id} catalog={catalog} toolbar={<>{back}{kind}</>} takeFocus={takeFocus}
        chatTitle={chatId => { const thread = threads.find(entry => entry.id === chatId); return thread ? chatTitle(thread) : undefined; }}
        onOpenPage={onOpenPage} onOpen={onOpenKnowledge} onAsk={onAsk} onChanged={onChanged}>
        {item.kind === "method" && item.learned ? <LensLearningPanel bridge={bridge} loupistId={loupistId} node={{ kind: "lens_method", recordId: item.id }} onArchived={onChanged} /> : null}
        <UsedIn usage={usage.get(item.id)} threads={threads} onOpenChat={onOpenChat} />
        {item.kind !== "perspective" ? <div className="memory-reader-actions">{pauseAction}<Button size="sm" variant="ghost" disabled={busy} onClick={() => void act(() => bridge.knowledgeLensArchive({ loupistId, recordId: item.id, archived: true }))}>{t("memoryPage.actions.archive")}</Button></div> : null}
      </PageReader>
    </aside>;
  }
  const principle = item.principle;
  const origin = principle.origin === "shared" ? null : principle.origin === "feedback" ? t("memoryPage.reader.originFeedback", { date: day(principle.createdAtMs) })
    : principle.origin === "chat" ? t("memoryPage.reader.originChat", { date: day(principle.createdAtMs) })
      : t("memoryPage.reader.originSettings", { date: day(principle.createdAtMs) });
  return <aside className="memory-reader" aria-label={item.title}>
    {back}{kind}{pausedHint}
    <p className="memory-reader-statement">{principle.statement}</p><PrincipleSource key={principle.revisionId} statement={principle.statement} why={principle.why ?? ""} example={principle.example ?? ""} />
    <SharedOrigin bridge={bridge} loupistId={loupistId} subjectId={item.id} onAbout={onAboutShared} />
    {principle.why ? <section className="memory-reader-section"><h3>{t("memoryPage.reader.why")}</h3><p>{principle.why}</p></section> : null}
    {principle.example ? <section className="memory-reader-section"><h3>{t("memoryPage.reader.example")}</h3><p>{principle.example}</p></section> : null}
    <UsedIn usage={usage.get(item.id)} threads={threads} onOpenChat={onOpenChat} />
    {origin ? <section className="memory-reader-section"><h3>{t("memoryPage.reader.origin")}</h3><p>{origin}</p>
      {principle.originChatId && threads.some(thread => thread.id === principle.originChatId) ? <Button variant="link" size="sm" className="memory-origin-chat" onClick={() => onOpenChat(principle.originChatId!)}>{t("memoryPage.reader.openChat")}</Button> : null}
    </section> : null}
    {error ? <p className="memory-reader-error" role="alert">{error}</p> : null}
    <div className="memory-reader-actions">
      {pauseAction}
      <Button size="sm" onClick={() => onAsk({ kind: "lens", loupistId, objectId: item.id, recordId: item.id, label: principle.statement })}>{t("lensWorkspace.discussInChat")}</Button>
      {onShare ? <Button size="sm" variant="outline" disabled={busy} data-share-item onClick={() => onShare(item.id)}>{i18n.t("sharing:share")}</Button> : null}
      <Button size="sm" variant="ghost" disabled={busy} onClick={() => void act(() => bridge.principleArchive({ loupistId, recordId: item.id, archived: true }))}>{t("memoryPage.actions.archive")}</Button>
    </div>
  </aside>;
}

function AboutYou({ bridge, loupist, data, onChanged, shared }: {
  bridge: LoupistBridge; loupist: LoupistSummary; data: Data; onChanged: () => void;
  /** "Shared Loupists you added", shown when any. */
  shared?: ReactNode;
}) {
  const [adding, setAdding] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [holdingsOpen, setHoldingsOpen] = useState(false);
  const addId = useId();
  const openAsset = useAssetPageOpener();
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true); setError(null);
    try { await action(); onChanged(); }
    catch (cause) { setError(errorMessage(cause, t("memoryPage.actionError"))); }
    finally { setBusy(false); }
  };
  const add = () => {
    const [symbol, market = "UNKNOWN"] = adding.trim().toUpperCase().split(":");
    if (!symbol) return;
    void run(async () => { await bridge.watchlistAdd({ symbol, market, loupistId: loupist.id }); setAdding(""); });
  };
  return <div className="memory-about">
    <section className="memory-card" aria-labelledby={addId}>
      <h2 id={addId}>{t("memoryPage.about.follow")}</h2>
      <ul className="memory-chips">
        {data.watchlist.map(entry => <li key={`${entry.symbol}:${entry.market}`}>
          {(() => {
            const label = followedName(entry, data.followedNodes) ? `${followedName(entry, data.followedNodes)} · ${entry.symbol}` : entry.symbol;
            // A followed asset opens its asset page.
            return openAsset ? <button type="button" className="memory-chip-open" onClick={() => openAsset({ loupistId: loupist.id, symbol: entry.symbol, market: entry.market, name: followedName(entry, data.followedNodes) ?? entry.displayName ?? undefined })}>{label}</button> : <span>{label}</span>;
          })()}
          <Button variant="ghost" size="icon-sm" disabled={busy} aria-label={t("memoryPage.about.remove", { name: followedName(entry, data.followedNodes) ?? entry.symbol })} onClick={() => void run(() => bridge.watchlistRemove({ symbol: entry.symbol, market: entry.market }))}><XIcon /></Button>
        </li>)}
      </ul>
      <form className="memory-follow-add" onSubmit={event => { event.preventDefault(); add(); }}>
        <Input aria-label={t("memoryPage.about.addLabel")} placeholder={t("memoryPage.about.addPlaceholder")} value={adding} onChange={event => setAdding(event.target.value)} />
        <Button type="submit" size="sm" variant="outline" disabled={busy || !adding.trim()}>{t("memoryPage.about.add")}</Button>
      </form>
      <p className="memory-meta memory-holdings" data-holdings-kinds>{holdingsLine(data.holdings)}
        <Button variant="link" size="sm" className="memory-holdings-update" onClick={() => setHoldingsOpen(true)}>{t("memoryPage.about.update")}</Button></p>
      <HoldingsSheet bridge={bridge} open={holdingsOpen} onOpenChange={open => { setHoldingsOpen(open); if (!open) onChanged(); }} />
    </section>
    {shared}
    {error ? <p className="memory-reader-error" role="alert">{error}</p> : null}
  </div>;
}


/** "Stocks and funds: 14 assets · Oct 12 · Crypto: 3 assets · Nov 1": counts and dates only, never quantities. */
function holdingsLine(holdings: HoldingsSummary | null) {
  if (!holdings || (!holdings.stocks && !holdings.crypto)) return t("memoryPage.about.noHoldings");
  const kind = (value: HoldingsSummary["stocks"]) => value
    ? t("memoryPage.about.kindCount", { count: value.count, formattedCount: formatNumber(value.count), date: day(value.importedAtMs) })
    : t("memoryPage.about.kindNone");
  return t("memoryPage.about.holdingsByKind", { stocks: kind(holdings.stocks), crypto: kind(holdings.crypto) });
}

const sameName = (left: string | null | undefined, right: string) => Boolean(left) && left!.trim().toLocaleLowerCase() === right.trim().toLocaleLowerCase();
/**
 * A followed asset by the same name the Map and the rest of the app use: the
 * Map's local name for its node when one matches, else the stored display name.
 */
function followedName(entry: WatchlistEntry, nodes: readonly KnowledgeMapNode[]) {
  const node = nodes.find(candidate => [candidate.label, ...(candidate.localNames ?? [])].some(name => sameName(entry.displayName, name) || sameName(entry.symbol, name)));
  return node ? entityLabel(node, i18n.language) : entry.displayName ?? undefined;
}

type HistoryRow = { key: string; atMs: number; text: string; failed?: boolean; itemId?: string; map?: boolean };

function sectionRows(page: PageView): HistoryRow[] {
  const heading = (key?: string) => key ? displaySectionHeading(key, (page.body.kind === "principle" ? undefined : page.body.sections.find(section => section.key === key)?.heading) ?? key) : "";
  return page.history.filter(entry => entry.origin === "user" && entry.outcome !== "summarized" && ["added", "retired"].includes(entry.outcome) && (entry.undoesOpId || entry.outcome === "retired")).map(entry => ({
    key: `section:${entry.opId}`, atMs: entry.createdAtMs, itemId: page.recordId,
    text: t(entry.undoesOpId ? "memoryPage.history.undone" : "memoryPage.history.forgot", { section: heading(entry.key), page: displayPageTitle(page.kind, page.title) }),
  }));
}

function historyRows(data: Data, threads: readonly ChatThread[]): HistoryRow[] {
  const titles = new Map(data.catalog.lenses.map(lens => [lens.recordId, lens.title]));
  const rows: HistoryRow[] = [...data.userRows];
  const filed = new Map<string, { atMs: number; chatId: string; recordIds: string[] }>();
  for (const event of data.events) {
    if ((event.kind !== "lens_created" && event.kind !== "lens_revised") || event.actor === "user" || !event.originChatId || !event.recordId) continue;
    const key = event.originMessageId ?? `${event.originChatId}:${event.atMs}`;
    const group = filed.get(key) ?? { atMs: event.atMs, chatId: event.originChatId, recordIds: [] };
    group.atMs = Math.max(group.atMs, event.atMs);
    if (!group.recordIds.includes(event.recordId)) group.recordIds.push(event.recordId);
    filed.set(key, group);
  }
  for (const [key, group] of filed) {
    const thread = threads.find(item => item.id === group.chatId);
    const first = group.recordIds.find(id => titles.has(id));
    rows.push({ key: `filed:${key}`, atMs: group.atMs, itemId: first,
      text: thread ? t("memoryPage.history.filedAfter", { chat: chatTitle(thread) }) : t("memoryPage.history.filed"), });
  }
  for (const event of data.events) {
    if (event.kind !== "schema_extended" && event.kind !== "schema_reduced") continue;
    const labels = event.origin && typeof event.origin === "object" && "labels" in event.origin ? event.origin.labels : undefined;
    const label = labels ? (i18n.language.toLowerCase().startsWith("ko") && labels.ko ? labels.ko : labels.en) : event.summary;
    rows.push({ key: event.eventId, atMs: event.atMs, map: true, text: t(event.kind === "schema_extended" ? "memoryPage.history.vocabularyAdded" : "memoryPage.history.vocabularyRemoved", { labels: label }) });
  }
  for (const review of data.reviews) {
    const title = review.lensRecordId ? titles.get(review.lensRecordId) : undefined;
    if (review.status === "created" || review.status === "refined") rows.push({ key: review.reviewId, atMs: review.createdAtMs, text: title ? t("memoryPage.history.learnedMethod", { title }) : t("memoryPage.history.learnedUntitled"), itemId: title ? review.lensRecordId ?? undefined : undefined });
    else if (review.status === "deferred") rows.push({ key: review.reviewId, atMs: review.createdAtMs, text: t("memoryPage.history.nothing") });
    else if (review.status === "failed") rows.push({ key: review.reviewId, atMs: review.createdAtMs, text: t("memoryPage.history.failed"), failed: true });
  }
  for (const principle of data.principles) rows.push({ key: `principle:${principle.recordId}`, atMs: principle.createdAtMs, text: t("memoryPage.history.taught", { statement: principle.statement }), itemId: principle.recordId });
  // Names added by one answer (or one day without a chat) form one row.
  const groups = new Map<string, { atMs: number; count: number; chatId: string | null }>();
  for (const event of data.events) {
    if (event.origin && typeof event.origin === "object" && "kind" in event.origin && event.origin.kind === "memory_reset" && !event.recordId) rows.push({ key: event.eventId, atMs: event.atMs, text: t("memoryPage.history.reset") });
    if (event.kind !== "entity_added") continue;
    const key = event.originMessageId ?? `day:${new Date(event.atMs).toDateString()}`;
    const group = groups.get(key) ?? { atMs: event.atMs, count: 0, chatId: event.originChatId };
    group.count += 1;
    group.atMs = Math.max(group.atMs, event.atMs);
    groups.set(key, group);
  }
  for (const [key, group] of groups) {
    const found = group.chatId ? threads.find(thread => thread.id === group.chatId) : undefined;
    const chat = found ? chatTitle(found) : undefined;
    rows.push({ key: `names:${key}`, atMs: group.atMs, map: true, text: chat
      ? t("memoryPage.history.mapAdded", { count: group.count, formattedCount: formatNumber(group.count), source: chat })
      : t("memoryPage.history.mapAddedPlain", { count: group.count, formattedCount: formatNumber(group.count) }) });
  }
  rows.sort((a, b) => b.atMs - a.atMs);
  return rows.slice(0, 200);
}

function History({ rows, selectedKey, onSelect }: { rows: readonly HistoryRow[]; selectedKey?: string; onSelect: (row: HistoryRow) => void }) {
  if (!rows.length) return <div className="memory-page-list"><p className="memory-empty">{t("memoryPage.history.empty")}</p></div>;
  const days: { label: string; rows: HistoryRow[] }[] = [];
  for (const row of rows) {
    const label = day(row.atMs);
    if (days.at(-1)?.label !== label) days.push({ label, rows: [] });
    days.at(-1)!.rows.push(row);
  }
  return <div className="memory-page-list memory-history">
    {days.map(group => <section key={group.label} aria-label={group.label}>
      <h2 className="memory-section-label">{group.label}</h2>
      <ul>{group.rows.map(row => <li key={row.key}>
        <button type="button" className="memory-history-row" aria-current={row.key === selectedKey || undefined} data-failed={row.failed || undefined} onClick={() => onSelect(row)}>
          <span>{row.text}</span><time>{formatTime(row.atMs)}</time>
        </button>
      </li>)}</ul>
    </section>)}
  </div>;
}

/** The reader for a history row that is not a principle or method: names added, or a review that learned nothing. */
function HistoryNote({ row, onOpenMap, onClose }: { row: HistoryRow; onOpenMap: () => void; onClose: () => void }) {
  return <article className="memory-reader memory-history-note" aria-label={row.text}>
    <Button className="memory-reader-back" variant="ghost" size="sm" onClick={onClose}><ArrowLeftIcon aria-hidden="true" />{t("memoryPage.tabs.history")}</Button>
    <p className="memory-history-note-date">{formatDateTime(row.atMs)}</p>
    <p className="memory-history-note-text">{row.text}</p>
    {row.failed ? <p className="memory-section-description">{t("memoryPage.history.failedDetail")}</p> : null}
    {row.map ? <Button variant="outline" size="sm" onClick={onOpenMap}>{t("memoryPage.history.openMap")}</Button> : null}
  </article>;
}
