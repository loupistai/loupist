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
import { HOST_STANDING_TITLE } from "../map/lensReading";
import { SharedOrigin } from "../sharing/SharedOrigin";
import { SHARED_SECTION_ITEM } from "../sharing/SharedPacks";
import { ShareSheet } from "../sharing/ShareSheet";
import { pinForLens } from "../../knowledge/location";
import "@/styles/page-reader.css";

const WORLD_LINK = /\]\(world:([^)\s]+)\)/gu;
const WORLD_PREFIX = "world~";
const SHARABLE: ReadonlySet<LensKind> = new Set(["principle", "perspective", "method"]);

/** A page's own title; the host's untouched standing title reads as the default perspective. */
export const pageTitle = (page: Pick<PageView, "title" | "kind">) =>
  page.kind === "perspective" && page.title === HOST_STANDING_TITLE ? i18n.t("knowledge:lensWorkspace.defaultPerspective") : page.title;

/** Rewrites `world:` links to indexed `lens:` links the markdown renderer already makes clickable. */
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
  /** Resolves Source chips to their retained record. */
  catalog?: KnowledgeCatalog;
  /** Left of the title: a back button and the page's use line. */
  toolbar?: React.ReactNode;
  /** After the history: "Used in", pause and archive. */
  children?: React.ReactNode;
  /** The chat's display title (the home Thread reads as the Bot). */
  chatTitle?: (chatId: string) => string | undefined;
  onOpenPage: (recordId: string) => void;
  /** Opens a Source, claim or Map name. */
  onOpen: (location: KnowledgeLocation) => void;
  onOpenLink?: (url: string) => void | Promise<void>;
  onAsk?: (pin: KnowledgePin) => void;
  onChanged?: () => void;
  /** Shown to the host when the page loads. */
  onLoaded?: (page: PageView) => void;
};

/** One Lens page: Markdown sections with their basis, what is contested, Forget with Undo, links and a short history. */
export function PageReader({ bridge, loupistId, recordId, catalog, toolbar, children, chatTitle, onOpenPage, onOpen, onOpenLink, onAsk, onChanged, onLoaded }: PageReaderProps) {
  const { t } = useTranslation("knowledge");
  const [page, setPage] = useState<PageView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [busy, setBusy] = useState(false);
  const [forgot, setForgot] = useState<{ opId: string; heading: string } | null>(null);
  const [allHistory, setAllHistory] = useState(false);
  const [shareOpen, setShareOpen] = useState(false);
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
    return () => { generation.current++; };
  }, [bridge, loupistId, recordId, reload]);
  useEffect(() => { if (page) heading.current?.focus({ preventScroll: true }); }, [page?.recordId]);

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

  const sections = page && page.body.kind !== "principle" ? page.body.sections : [];
  const rewritten = useMemo(() => {
    const ids: string[] = [];
    return { texts: sections.map(section => withMapLinks(section.markdown, ids)), ids };
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
      case "span": return catalog?.sources.some(item => item.recordId === ref.recordId)
        ? <button key={index} type="button" className="page-chip" data-chip="source" onClick={() => openSource(ref)}>{t("pageReader.chip.source")}</button>
        : <span key={index} className="page-chip" data-chip="source">{t("pageReader.chip.source")}</span>;
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
      <p className="page-reader-meta">{toolbar ? null : <span>{t(`pageReader.kind.${page.kind}`)}</span>}{page.updatedAtMs ? <span>{t("pageReader.updated", { date: formatDate(page.updatedAtMs, { year: undefined, month: "short", day: "numeric" }) })}</span> : null}
        {idea ? <span className="page-idea-status" data-status={idea.status}>{t(`pageReader.ideaStatus.${idea.status}`)}</span> : null}</p>
      <h2 ref={heading} tabIndex={-1}>{pageTitle(page)}</h2>
      <SharedOrigin bridge={bridge} loupistId={loupistId} subjectId={page.recordId} onAbout={() => onOpen({ loupistId, surface: "lens", recordId: SHARED_SECTION_ITEM })} />
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
            {page.archived ? null : <Button variant="ghost" size="sm" disabled={busy} aria-label={t("pageReader.forgetSection", { heading: section.heading })} onClick={() => forget(section)}>{t("pageReader.forget")}</Button>}
          </div>
        </header>
        <SafeMessageResponse text={rewritten.texts[index] ?? section.markdown} isAnimating={false} onOpenLink={onOpenLink ?? (() => undefined)} onLensLink={openLens} />
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
    {changed.length ? <section className="page-history" aria-label={t("pageReader.history.label")}><h3>{t("pageReader.history.label")}</h3>
      <ul>{shownHistory.map(entry => <li key={entry.opId}><span>{entryText(entry)}</span><time>{formatDate(entry.createdAtMs, { year: undefined, month: "short", day: "numeric" })}</time></li>)}</ul>
      {changed.length > 5 ? <Button variant="link" size="sm" aria-expanded={allHistory} onClick={() => setAllHistory(value => !value)}>{allHistory ? t("pageReader.history.less") : t("pageReader.history.more", { count: changed.length - 5, formattedCount: formatNumber(changed.length - 5) })}</Button> : null}
    </section> : null}
    <div className="page-reader-actions">
      {onAsk ? <Button variant="outline" onClick={() => onAsk(pinForLens(loupistId, page))}><MessageSquareIcon />{t("lensWorkspace.discussInChat")}</Button> : null}
      {SHARABLE.has(page.kind) && page.kind !== "perspective" ? <Button variant="outline" size="sm" data-share-item onClick={() => setShareOpen(true)}>{i18n.t("sharing:share")}</Button> : null}
    </div>
    {children}
    <ShareSheet bridge={bridge} loupistId={loupistId} loupistName="" scope={{ kind: "item", recordId: page.recordId }} open={shareOpen} onOpenChange={setShareOpen} />
  </article>;
}
