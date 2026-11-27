import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { MessageSquareIcon, TriangleAlertIcon } from "lucide-react";
import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import type { BasisRefView, KnowledgeCatalog, KnowledgeLocation, KnowledgePin, LensKind, LensLogEntryView, LoupistBridge, PageView, SectionView } from "../../types";
import { errorMessage } from "../../lib/errors";
import { Alert, AlertDescription } from "../ui/alert";
import { Button } from "../ui/button";
import { useRequestTurn } from "./requestTurn";
import { SectionSources } from "./SectionSources";
import { Spinner } from "../ui/spinner";
import { SafeMessageResponse } from "../chat/SafeMessageResponse";
import { OpenProvider, useOpenLayer, type OpenLayer } from "../navigation/Open";
import { displaySectionHeading, historySectionHeading } from "../../knowledge/hostDefaults";
import { dateValue } from "../../knowledge/presentation";
import { sectionRoleLabel, sectionState, supersededChipLabel } from "./sectionState";
import { historyEpisode } from "../map/claimHistory";
import { sharingReleaseVisible } from "../../sharing/releaseVisibility";
import { SharedOrigin } from "../sharing/SharedOrigin";
import { SHARED_SECTION_ITEM } from "../sharing/SharedPacks";
import { ShareSheet } from "../sharing/ShareSheet";
import { pinForLens } from "../../knowledge/location";
import { LearnedMark } from "../knowledge/LearnedMark";
import { LearnRequest } from "../briefings/LearnRequest";
import "@/styles/page-reader.css";

const WORLD_LINK = /\]\(world:([^)\s]+)\)/gu;
const WORLD_PREFIX = "world~";
const SHARABLE: ReadonlySet<LensKind> = new Set(["principle", "perspective", "method"]);

function withMapLinks(markdown: string, nodeIds: string[]) {
  return markdown.replace(WORLD_LINK, (_, nodeId: string) => {
    let index = nodeIds.indexOf(nodeId);
    if (index < 0) index = nodeIds.push(nodeId) - 1;
    return `](lens:${WORLD_PREFIX}${index})`;
  });
}

export type PageReaderProps = {
  bridge: LoupistBridge;
  loupistId: string;
  recordId: string;
  catalog?: KnowledgeCatalog;
  toolbar?: React.ReactNode;
  /** How often the page was used, said once in the meta line beside its kind and date. */
  usage?: string;
  children?: React.ReactNode;
  chatTitle?: (chatId: string) => string | undefined;
  onOpenPage: (recordId: string) => void;
  onOpen: (location: KnowledgeLocation) => void;
  onAsk?: (pin: KnowledgePin) => void;
  onChanged?: () => void;
  onLoaded?: (page: PageView) => void;
  /** False for a page shown without being asked for, so it never takes focus. */
  takeFocus?: boolean;
};

/** A Lens link in a page moves this reader (a Map anchor opens the Map); other links open as anywhere else. */
function PageLinks({ onLens, children }: { onLens: (recordId: string) => void; children: React.ReactNode }) {
  const open = useOpenLayer(useCallback<OpenLayer>((to) => to.kind === "lens" ? () => onLens(to.recordId) : undefined, [onLens]));
  return <OpenProvider open={open}>{children}</OpenProvider>;
}

function SupersededChip({ bridge, loupistId, claimId, fetch, onOpen }: {
  bridge: LoupistBridge; loupistId: string; claimId?: string; fetch: boolean; onOpen: (claimId: string) => void;
}) {
  const { t } = useTranslation("knowledge");
  const [label, setLabel] = useState(() => t("pageReader.superseded"));
  useEffect(() => {
    if (!fetch || !claimId) return;
    let live = true;
    setLabel(t("pageReader.superseded"));
    void bridge.knowledgeWorldClaimHistory({ loupistId, claimId }).then(history => {
      if (!live) return;
      const episode = historyEpisode(history.ended);
      setLabel(supersededChipLabel({ end: episode?.date, source: episode?.title }));
    }).catch(() => { if (live) setLabel(t("pageReader.superseded")); });
    return () => { live = false; };
  }, [bridge, loupistId, claimId, fetch, t]);
  return <button type="button" className="page-contested" data-chip="superseded" disabled={!claimId} onClick={() => claimId && onOpen(claimId)}>{label}</button>;
}

export function PageReader({ bridge, loupistId, recordId, catalog, toolbar, usage, children, chatTitle, onOpenPage, onOpen, onAsk, onChanged, onLoaded, takeFocus = true }: PageReaderProps) {
  const { t } = useTranslation("knowledge");
  const requestTurn = useRequestTurn();
  const [page, setPage] = useState<PageView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [busy, setBusy] = useState(false);
  const [forgot, setForgot] = useState<{ opId: string; heading: string } | null>(null);
  const [allHistory, setAllHistory] = useState(false);
  const [shareOpen, setShareOpen] = useState(false);
  const showSharing = sharingReleaseVisible();
  const generation = useRef(0);
  const heading = useRef<HTMLHeadingElement>(null);
  const pageRef = useRef(page);
  pageRef.current = page;

  useEffect(() => {
    const ticket = ++generation.current;
    setPage(null); setLoadError(null); setActionError(null); setForgot(null); setAllHistory(false);
    void bridge.knowledgeLensRead({ loupistId, recordId }).then(view => {
      if (ticket !== generation.current) return;
      setPage(view);
      onLoaded?.(view);
    }).catch((cause: unknown) => { if (ticket === generation.current) setLoadError(errorMessage(cause, t("pageReader.loadError"))); });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a request counter, not a DOM node: cleanup invalidates the request that is in flight.
    return () => { generation.current++; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- loads once per record; `t` only words the failure and `onLoaded` is the parent's inline callback.
  }, [bridge, loupistId, recordId, reload]);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- focuses the heading once per page.
  useEffect(() => { if (page && takeFocus) heading.current?.focus({ preventScroll: true }); }, [page?.recordId, takeFocus]);

  const act = useCallback(async (run: () => Promise<PageView>, after?: (next: PageView) => void) => {
    if (busy) return;
    setBusy(true); setActionError(null);
    try {
      const next = await run();
      if (pageRef.current?.recordId === next.recordId) setPage(next);
      after?.(next);
      onChanged?.();
    } catch (cause) { setActionError(errorMessage(cause, t("pageReader.actionError"))); }
    finally { setBusy(false); }
  }, [busy, onChanged, t]);

  const sections = useMemo(() => page && page.body.kind !== "principle" ? page.body.sections.map(section => ({ ...section, heading: displaySectionHeading(section.key, section.heading) })) : [], [page]);
  const rewritten = useMemo(() => {
    const ids: string[] = [];
    return { texts: sections.map(section => withMapLinks(section.markdown, ids)), ids };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `sections` derive from the page.
  }, [page]);

  if (loadError) return <Alert variant="destructive"><AlertDescription>{loadError}</AlertDescription><Button variant="outline" onClick={() => setReload(value => value + 1)}>{t("pageReader.retry")}</Button></Alert>;
  if (!page) return <p role="status" className="memory-loading-inline"><Spinner />{t("pageReader.opening")}</p>;

  const body = page.body;
  const known = new Map([...page.links, ...page.backlinks].map(link => [link.recordId, link.title]));
  const openMap = (nodeId: string) => onOpen({ loupistId, surface: "world", selection: { kind: "node", id: nodeId } });
  const openLens = (target: string) => {
    if (target.startsWith(WORLD_PREFIX)) {
      const nodeId = rewritten.ids[Number(target.slice(WORLD_PREFIX.length))];
      if (nodeId) openMap(nodeId);
      return;
    }
    onOpenPage(target);
  };
  const openClaim = (claimId: string) => onOpen({ loupistId, surface: "world", selection: { kind: "claim", id: claimId } });
  const forget = (section: SectionView) => void act(() => bridge.memorySectionForget({ loupistId, recordId, key: section.key }), next => {
    const entry = next.history.find(item => item.op === "retire" && item.key === section.key);
    setForgot(entry ? { opId: entry.opId, heading: section.heading } : null);
  });
  const undo = (opId: string) => void act(() => bridge.memoryOpUndo({ loupistId, opId }), () => setForgot(null));

  const chip = (ref: Exclude<BasisRefView, { kind: "span" }>, index: number, section: SectionView) => {
    switch (ref.kind) {
      case "claim": return <button key={index} type="button" className="page-chip" data-chip="claim" onClick={() => openClaim(ref.claimId)}>{t("pageReader.chip.claim")}</button>;
      case "said": {
        const turn = requestTurn?.(ref.messageId);
        const date = formatDate(turn ? new Date(turn.date) : section.updatedAtMs, { year: undefined, month: "short", day: "numeric" });
        const label = t("pageReader.chip.requested", { date });
        return turn ? <button key={index} type="button" role="link" className="page-chip" data-chip="said" onClick={turn.open}>{label}</button>
          : <span key={index} className="page-chip" data-chip="said">{label}</span>;
      }
      case "page": {
        // A page title can be a whole sentence: the chip shortens it and keeps it whole in its name and tooltip.
        const label = t("pageReader.chip.page", { title: known.get(ref.recordId) ?? t("pageReader.chip.pageUnknown") });
        return <button key={index} type="button" className="page-chip" data-chip="page" title={label} onClick={() => onOpenPage(ref.recordId)}><span className="page-chip-label">{label}</span></button>;
      }
    }
  };

  const entryText = (entry: LensLogEntryView) => {
    const section = historySectionHeading(entry);
    const chat = entry.chatId ? chatTitle?.(entry.chatId) : undefined;
    const what = entry.undoesOpId ? t("pageReader.history.undone", { section })
      : entry.op === "summary" ? t("pageReader.history.summary")
        : entry.op === "idea_status" ? t("pageReader.history.status")
          : entry.outcome === "added" ? t("pageReader.history.added", { section })
            : entry.outcome === "replaced" ? t("pageReader.history.replaced", { section })
              : entry.origin === "user" ? t("pageReader.history.forgot", { section }) : t("pageReader.history.retired", { section });
    return chat ? t("pageReader.history.after", { what, chat }) : what;
  };
  const changed = page.history.filter(entry => ["added", "replaced", "retired", "summarized", "status_set"].includes(entry.outcome));
  const shownHistory = allHistory ? changed : changed.slice(0, 5);
  const anchors = page.anchors.filter((anchor, index, all) => all.findIndex(other => other.nodeId === anchor.nodeId && other.name === anchor.name) === index);
  const idea = body.kind === "idea" ? body.idea : null;

  const changedSections = sections.filter(section => sectionState(section) !== "current");
  const scrollToChanged = (event: MouseEvent<HTMLButtonElement>) => {
    event.currentTarget.closest("article")?.querySelector<HTMLElement>('[data-section-state="contested"], [data-section-state="superseded"]')?.scrollIntoView({ block: "start" });
  };
  return <article className="page-reader" aria-label={t("pageReader.label")} data-page-kind={page.kind}>
    {toolbar ? <div className="page-reader-toolbar">{toolbar}</div> : null}
    <header className="page-reader-head">
      <p className="page-reader-meta"><span>{t(`pageReader.kind.${page.kind}`)}</span>{usage ? <span>{usage}</span> : null}{page.updatedAtMs ? <span>{t("pageReader.updated", { date: formatDate(page.updatedAtMs, { year: undefined, month: "short", day: "numeric" }) })}</span> : null}
        {idea ? <span className="page-idea-status" data-status={idea.status}>{t(`pageReader.ideaStatus.${idea.status}`)}</span> : null}</p>
      <h2 ref={heading} tabIndex={-1}>{page.title}</h2>
      {changedSections.length ? <p className="page-changed"><Button variant="link" onClick={scrollToChanged}>{t("pageReader.changedSections", { count: changedSections.length, formattedCount: formatNumber(changedSections.length) })}</Button></p> : null}
      <SharedOrigin bridge={bridge} loupistId={loupistId} subjectId={page.recordId} onAbout={showSharing ? () => onOpen({ loupistId, surface: "lens", recordId: SHARED_SECTION_ITEM }) : undefined} />
      {body.kind !== "principle" && body.summary ? <p className="page-reader-summary">{body.summary}</p> : null}
    </header>
    {actionError ? <Alert variant="destructive"><AlertDescription>{actionError}</AlertDescription></Alert> : null}
    {forgot ? <p className="page-forgot" role="status">{t("pageReader.forgot", { heading: forgot.heading })}<Button variant="link" size="sm" disabled={busy} onClick={() => undo(forgot.opId)}>{t("pageReader.undo")}</Button></p> : null}
    {body.kind === "principle" ? <div className="page-sections">
      <p className="page-principle-statement">{body.statement}</p>
      {body.why ? <section className="page-section"><h3>{t("pageReader.why")}</h3><p>{body.why}</p></section> : null}
      {body.example ? <section className="page-section"><h3>{t("pageReader.example")}</h3><p>{body.example}</p></section> : null}
    </div> : <div className="page-sections">
      {sections.map((section, index) => {
        const state = sectionState(section);
        const role = sectionRoleLabel(section.role);
        const supersededIndex = state === "superseded" ? sections.slice(0, index).filter(item => sectionState(item) === "superseded").length : 0;
        return <section key={section.key} className="page-section" data-section-key={section.key} data-section-state={state} data-contested={section.contested || undefined} aria-label={section.heading.trim() || undefined}>
        {role ? <p className="page-section-role">{role}</p> : null}
        {section.heading.trim() && <h3>{section.heading}</h3>}
        {section.asOf ? <p className="page-as-of">{t("pageReader.asOf", { date: dateValue(section.asOf) })}</p> : null}
        <PageLinks onLens={openLens}><SafeMessageResponse text={rewritten.texts[index] ?? section.markdown} isAnimating={false} /></PageLinks>
        {section.chip === "view" && section.basis.uncertainty ? <p className="page-uncertainty">{t("pageReader.uncertainty", { text: section.basis.uncertainty })}</p> : null}
        <div className="page-section-tools">
          <SectionSources refs={section.basis.refs.filter(ref => ref.kind === "span")} catalog={catalog} loupistId={loupistId} onOpen={onOpen} />
          {section.chip === "view" ? <span className="page-chip" data-chip="view">{t("pageReader.chip.view")}</span> : null}
          <LearnedMark learned={section.learned} />
          {section.basis.refs.filter(ref => ref.kind !== "span").map((ref, index) => chip(ref, index, section))}
          {section.contested ? <button type="button" className="page-contested" disabled={!section.contestedClaimIds[0]} onClick={() => section.contestedClaimIds[0] && openClaim(section.contestedClaimIds[0])}><TriangleAlertIcon aria-hidden="true" />{t("pageReader.contested")}</button> : null}
          {state === "superseded" ? <SupersededChip bridge={bridge} loupistId={loupistId} claimId={section.claimIds?.[0]} fetch={supersededIndex < 16} onOpen={openClaim} /> : null}
          {page.archived ? null : <Button variant="ghost" size="sm" className="page-forget" disabled={busy} aria-label={section.heading.trim() ? t("pageReader.forgetSection", { heading: section.heading }) : t("pageReader.forget")} onClick={() => forget(section)}>{t("pageReader.forget")}</Button>}
        </div>
      </section>; })}
      {!sections.length ? <p className="knowledge-hint">{t(page.kind === "perspective" ? "pageReader.emptyPerspective" : "pageReader.emptyPage")}</p> : null}
    </div>}
    {anchors.length ? <section className="page-linked" aria-label={t("pageReader.onTheMap")}><h3>{t("pageReader.onTheMap")}</h3>
      <ul className="page-chips">{anchors.map(anchor => <li key={`${anchor.nodeId ?? ""}:${anchor.name}`}>{anchor.nodeId
        ? <button type="button" className="page-chip" data-chip="map" onClick={() => openMap(anchor.nodeId!)}>{anchor.name}</button>
        : <span className="page-chip" data-chip="map">{anchor.name}</span>}</li>)}</ul></section> : null}
    {page.links.length || page.backlinks.length ? <section className="page-linked" aria-label={t("pageReader.relatedPages")}>
      {page.links.length ? <><h3>{t("pageReader.links")}</h3><ul className="page-chips">{page.links.map(link => <li key={link.recordId}><button type="button" className="page-chip" data-chip="page" onClick={() => onOpenPage(link.recordId)}>{link.title}</button></li>)}</ul></> : null}
      {page.backlinks.length ? <><h3>{t("pageReader.backlinks")}</h3><ul className="page-chips">{page.backlinks.map(link => <li key={link.recordId}><button type="button" className="page-chip" data-chip="page" onClick={() => onOpenPage(link.recordId)}>{link.title}</button></li>)}</ul></> : null}
    </section> : null}
    {changed.length ? <section className="page-history" data-testid="page-history" aria-label={t("pageReader.history.label")}><h3>{t("pageReader.history.label")}</h3>
      <ul>{shownHistory.map(entry => <li key={entry.opId}><span>{entryText(entry)}</span><time>{formatDate(entry.createdAtMs, { year: undefined, month: "short", day: "numeric" })}</time></li>)}</ul>
      {changed.length > 5 ? <Button variant="link" size="sm" aria-expanded={allHistory} onClick={() => setAllHistory(value => !value)}>{allHistory ? t("pageReader.history.less") : t("pageReader.history.more", { count: changed.length - 5, formattedCount: formatNumber(changed.length - 5) })}</Button> : null}
    </section> : null}
    <div className="page-reader-actions">
      {onAsk ? <Button variant="outline" size="sm" onClick={() => onAsk(pinForLens(loupistId, page))}><MessageSquareIcon />{t("lensWorkspace.discussInChat")}</Button> : null}
      {page.kind === "subject" && !page.archived ? <LearnRequest kind="lens" subject={page.title} /> : null}
      {showSharing && SHARABLE.has(page.kind) && page.kind !== "perspective" ? <Button variant="outline" size="sm" data-share-item onClick={() => setShareOpen(true)}>{i18n.t("sharing:share")}</Button> : null}
    </div>
    {children}
    {showSharing ? <ShareSheet bridge={bridge} loupistId={loupistId} loupistName="" scope={{ kind: "item", recordId: page.recordId }} open={shareOpen} onOpenChange={setShareOpen} /> : null}
  </article>;
}
