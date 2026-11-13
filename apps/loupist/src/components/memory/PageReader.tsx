import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { MessageSquareIcon, TriangleAlertIcon } from "lucide-react";
import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import type { BasisRefView, KnowledgeCatalog, KnowledgeLocation, KnowledgePin, LensKind, LensLogEntryView, LoupistBridge, PageView, SectionView } from "../../types";
import { errorMessage } from "../../lib/errors";
import { Alert, AlertDescription } from "../ui/alert";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";
import { SafeMessageResponse } from "../chat/SafeMessageResponse";
import { OpenProvider, useOpenLayer, type OpenLayer } from "../navigation/Open";
import { displayPageTitle, displaySectionHeading } from "../../knowledge/hostDefaults";
import { sharingReleaseVisible } from "../../sharing/releaseVisibility";
import { SharedOrigin } from "../sharing/SharedOrigin";
import { SHARED_SECTION_ITEM } from "../sharing/SharedPacks";
import { ShareSheet } from "../sharing/ShareSheet";
import { pinForLens } from "../../knowledge/location";
import { sourceTitle } from "../../knowledge/hostSource";
import "@/styles/page-reader.css";

const WORLD_LINK = /\]\(world:([^)\s]+)\)/gu;
const WORLD_PREFIX = "world~";
const SHARABLE: ReadonlySet<LensKind> = new Set(["principle", "perspective", "method"]);

export const pageTitle = (page: Pick<PageView, "title" | "kind">) => displayPageTitle(page.kind, page.title);

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

export function PageReader({ bridge, loupistId, recordId, catalog, toolbar, usage, children, chatTitle, onOpenPage, onOpen, onAsk, onChanged, onLoaded, takeFocus = true }: PageReaderProps) {
  const { t } = useTranslation("knowledge");
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
  const openMap = (nodeId: string) => onOpen({ loupistId, surface: "world", view: { followCurrent: true }, selection: { kind: "node", id: nodeId } });
  const openLens = (target: string) => {
    if (target.startsWith(WORLD_PREFIX)) {
      const nodeId = rewritten.ids[Number(target.slice(WORLD_PREFIX.length))];
      if (nodeId) openMap(nodeId);
      return;
    }
    onOpenPage(target);
  };
  const openSource = (ref: Extract<BasisRefView, { kind: "span" }>) => {
    const record = catalog?.sources.find(item => item.recordId === ref.recordId);
    const sourceId = record?.objects[0]?.id;
    if (!record) return;
    onOpen({ loupistId, surface: "source", recordId: record.recordId, revisionId: record.revisionId, objectId: sourceId, selection: { kind: "source", sourceId, spanId: ref.spanId } });
  };
  const openClaim = (claimId: string) => onOpen({ loupistId, surface: "world", view: { followCurrent: true }, selection: { kind: "claim", id: claimId } });
  const forget = (section: SectionView) => void act(() => bridge.memorySectionForget({ loupistId, recordId, key: section.key }), next => {
    const entry = next.history.find(item => item.op === "retire" && item.key === section.key);
    setForgot(entry ? { opId: entry.opId, heading: section.heading } : null);
  });
  const undo = (opId: string) => void act(() => bridge.memoryOpUndo({ loupistId, opId }), () => setForgot(null));

  const chip = (ref: BasisRefView, index: number) => {
    switch (ref.kind) {
      case "claim": return <button key={index} type="button" className="page-chip" data-chip="claim" onClick={() => openClaim(ref.claimId)}>{t("pageReader.chip.claim")}</button>;
      case "span": {
        // "Source · {title}" when the Library has the source, else just "Source" (direction N3).
        const record = catalog?.sources.find(item => item.recordId === ref.recordId);
        const title = record ? sourceTitle(record).trim() : "";
        const label = title ? t("pageReader.chip.sourceTitled", { title }) : t("pageReader.chip.source");
        return record
          ? <button key={index} type="button" className="page-chip" data-chip="source" title={title || undefined} onClick={() => openSource(ref)}><span className="page-chip-label">{label}</span></button>
          : <span key={index} className="page-chip" data-chip="source">{label}</span>;
      }
      case "said": return <span key={index} className="page-chip" data-chip="said">{t("pageReader.chip.said")}</span>;
      case "page": return <button key={index} type="button" className="page-chip" data-chip="page" onClick={() => onOpenPage(ref.recordId)}>{t("pageReader.chip.page", { title: known.get(ref.recordId) ?? t("pageReader.chip.pageUnknown") })}</button>;
    }
  };

  const entryText = (entry: LensLogEntryView) => {
    const section = entry.key ? sections.find(item => item.key === entry.key)?.heading ?? entry.key : "";
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

  return <article className="page-reader" aria-label={t("pageReader.label")} data-page-kind={page.kind}>
    {toolbar ? <div className="page-reader-toolbar">{toolbar}</div> : null}
    <header className="page-reader-head">
      <p className="page-reader-meta"><span>{t(`pageReader.kind.${page.kind}`)}</span>{usage ? <span>{usage}</span> : null}{page.updatedAtMs ? <span>{t("pageReader.updated", { date: formatDate(page.updatedAtMs, { year: undefined, month: "short", day: "numeric" }) })}</span> : null}
        {idea ? <span className="page-idea-status" data-status={idea.status}>{t(`pageReader.ideaStatus.${idea.status}`)}</span> : null}</p>
      <h2 ref={heading} tabIndex={-1}>{pageTitle(page)}</h2>
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
      {sections.map((section, index) => <section key={section.key} className="page-section" data-section-key={section.key} data-contested={section.contested || undefined} aria-label={section.heading}>
        <header>
          <h3>{section.heading}</h3>
          <div className="page-section-tools">
            {section.chip === "view" ? <span className="page-chip" data-chip="view">{t("pageReader.chip.view")}</span> : null}
            {section.basis.refs.map(chip)}
            {section.contested ? <button type="button" className="page-contested" disabled={!section.contestedClaimIds[0]} onClick={() => section.contestedClaimIds[0] && openClaim(section.contestedClaimIds[0])}><TriangleAlertIcon aria-hidden="true" />{t("pageReader.contested")}</button> : null}
            {page.archived ? null : <Button variant="ghost" size="sm" className="page-forget" disabled={busy} aria-label={t("pageReader.forgetSection", { heading: section.heading })} onClick={() => forget(section)}>{t("pageReader.forget")}</Button>}
          </div>
        </header>
        <PageLinks onLens={openLens}><SafeMessageResponse text={rewritten.texts[index] ?? section.markdown} isAnimating={false} /></PageLinks>
        {section.chip === "view" && section.basis.uncertainty ? <p className="page-uncertainty">{t("pageReader.uncertainty", { text: section.basis.uncertainty })}</p> : null}
      </section>)}
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
      {onAsk ? <Button variant="outline" onClick={() => onAsk(pinForLens(loupistId, page))}><MessageSquareIcon />{t("lensWorkspace.discussInChat")}</Button> : null}
      {showSharing && SHARABLE.has(page.kind) && page.kind !== "perspective" ? <Button variant="outline" size="sm" data-share-item onClick={() => setShareOpen(true)}>{i18n.t("sharing:share")}</Button> : null}
    </div>
    {children}
    {showSharing ? <ShareSheet bridge={bridge} loupistId={loupistId} loupistName="" scope={{ kind: "item", recordId: page.recordId }} open={shareOpen} onOpenChange={setShareOpen} /> : null}
  </article>;
}
