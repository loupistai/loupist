import { formatNumber } from "../../i18n/format";
import i18n, { useTranslation } from "../../i18n";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ListTreeIcon, MessageSquareIcon } from "lucide-react";
import { bodyWithoutRepeatedTitle } from "../../knowledge/reading";
import { Alert, AlertDescription } from "../ui/alert";
import { Button } from "../ui/button";
import { EmptyState } from "../ui/empty-state";
import { Spinner } from "../ui/spinner";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../ui/dialog";
import type { LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin, LensChangeRecord, LensDocumentBody, LensProposal, LensView } from "../../types";
import type { LensLinkActions } from "../ai-elements/message-response";
import { errorMessage } from "../../lib/errors";
import { knowledgeLocationFromReference, pinForLens } from "../../knowledge/location";
import { matchLensProposal, proposalMatchError } from "../../chat/dismissLensProposal";
import { SafeMessageResponse } from "../chat/SafeMessageResponse";
import { HOST_STANDING_TITLE, isStandingPlaceholder } from "../map/lensReading";
import { LensContextPane, markdownOutline, type LensOutlineItem } from "./LensContextPane";
import { ShareSheet } from "../sharing/ShareSheet";
import { SharedOrigin } from "../sharing/SharedOrigin";
import { SHARED_SECTION_ITEM } from "../sharing/SharedPacks";

const evidenceLabel = (kind: string, index: number) => i18n.t(kind === "source" ? "knowledge:lensWorkspace.sourceEvidence" : kind.startsWith("world") ? "knowledge:lensWorkspace.worldEvidence" : kind === "journal" ? "knowledge:lensWorkspace.judgmentEvidence" : kind === "research" ? "knowledge:lensWorkspace.noteEvidence" : "knowledge:lensWorkspace.lensEvidence", { index: formatNumber(index + 1) });
const fields = () => [
  ["applicability", i18n.t("knowledge:lensWorkspace.whenToApply"), i18n.t("knowledge:lensWorkspace.theQuestionsOrSituationsWhereThisLensHelps")],
  ["assumptions", i18n.t("knowledge:lensWorkspace.assumptions"), i18n.t("knowledge:lensWorkspace.whatThisPerspectiveTakesAsGiven")],
  ["procedure", i18n.t("knowledge:lensWorkspace.procedure"), i18n.t("knowledge:lensWorkspace.howToExamineTheEvidence")],
  ["principles", i18n.t("knowledge:lensWorkspace.principles"), i18n.t("knowledge:lensWorkspace.whatMattersWhenInterpretingTheEvidence")],
  ["limitations", i18n.t("knowledge:lensWorkspace.limitsAndCounterevidence"), i18n.t("knowledge:lensWorkspace.whereItStopsHelpingAndWhatWouldChangeYourMind")],
] as const;

function markdownText(markdown: string, sections: [string, string][]) {
  const populated = sections.filter(([, value]) => value.trim());
  return [markdown, ...populated.map(([label, value]) => `## ${label}\n\n${value}`)].filter(Boolean).join("\n\n");
}

export function LensMarkdownSource({ markdown, sections = [] }: { markdown: string; sections?: [string, string][] }) {
  const { t } = useTranslation("knowledge");
  const [open, setOpen] = useState(false);
  return <details className="lens-basis lens-markdown-source" onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>{t("lensWorkspace.markdownSource")}</summary>
    {open && <pre>{markdownText(markdown, sections)}</pre>}
  </details>;
}

function LensDocumentContent({ title, document, onOpenLink, onLensLink, lensLinks, children }: { title: string; document: LensDocumentBody; onOpenLink: (url: string) => void | Promise<void>; onLensLink: (recordId: string) => void; lensLinks?: LensLinkActions; children?: ReactNode }) {
  const { t } = useTranslation("knowledge");
  const body = bodyWithoutRepeatedTitle(title, document.markdown).trim();
  const sections = fields().filter(([key]) => key !== "applicability" && key !== "limitations" && document[key].trim());
  return <>
    {document.applicability.trim() || document.limitations.trim() ? <section className="lens-brief" aria-label={t("lensWorkspace.lensAtAGlance")}><dl>
      {document.applicability.trim() ? <div data-lens-section="applicability"><dt>{t("lensWorkspace.whenToUseIt")}</dt><dd><SafeMessageResponse text={document.applicability} isAnimating={false} onOpenLink={onOpenLink} onLensLink={onLensLink} lensLinks={lensLinks} /></dd></div> : null}
      {document.limitations.trim() ? <div data-lens-section="limitations"><dt>{t("lensWorkspace.whatToQuestion")}</dt><dd><SafeMessageResponse text={document.limitations} isAnimating={false} onOpenLink={onOpenLink} onLensLink={onLensLink} lensLinks={lensLinks} /></dd></div> : null}
    </dl></section> : null}
    {children || body || sections.length ? <div className="lens-document-body">
      {children}
      {body ? <div className="lens-main-content" data-lens-section="markdown"><SafeMessageResponse text={body} isAnimating={false} onOpenLink={onOpenLink} onLensLink={onLensLink} lensLinks={lensLinks} /></div> : null}
      {sections.length ? <div className="lens-document-sections">{sections.map(([key, label]) => <section key={key} data-lens-section={key}><h3>{label}</h3><SafeMessageResponse text={document[key]} isAnimating={false} onOpenLink={onOpenLink} onLensLink={onLensLink} lensLinks={lensLinks} /></section>)}</div> : null}
    </div> : null}
  </>;
}

type Props = { markdownFirst?: boolean; children?: ReactNode; bridge: LoupistBridge; catalog: KnowledgeCatalog; location: KnowledgeLocation; onOpen: (location: KnowledgeLocation) => void; onPatch?: (location: KnowledgeLocation) => void; onAsk: (pin: KnowledgePin, threadId?: string) => void; onChanged: () => void; onOpenLink?: (url: string) => void | Promise<void> };

export function LensWorkspace({ markdownFirst = false, children, bridge, catalog, location, onOpen, onPatch, onAsk, onChanged, onOpenLink }: Props) {
  const { t } = useTranslation("knowledge");
  const [showMarkdown, setShowMarkdown] = useState(markdownFirst);
  const record = location.recordId ? catalog.lenses.find((item) => item.recordId === location.recordId) : catalog.lenses.find((item) => item.lensKind === "perspective") ?? catalog.lenses[0];
  const recordId = location.recordId ?? record?.recordId;
  const revisionId = location.revisionId ?? record?.revisionId;
  const [server, setServer] = useState<LensView | null>(null);
  const [shareOpen, setShareOpen] = useState(false);
  const [basis, setBasis] = useState<LensChangeRecord[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [loadingBasis, setLoadingBasis] = useState(false);
  const [basisError, setBasisError] = useState<string | null>(null);
  const generation = useRef(0);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const basisGeneration = useRef(0);
  const scope = `${location.loupistId}:${recordId ?? ""}:${revisionId ?? ""}`;
  const scopeRef = useRef(scope);
  scopeRef.current = scope;
  const readerRef = useRef<HTMLHeadingElement | null>(null);
  const readerScrollRef = useRef<HTMLDivElement | null>(null);
  const workspaceRef = useRef<HTMLDivElement | null>(null);
  const [compact, setCompact] = useState(false);
  const [wide, setWide] = useState(false);
  const [contextOpen, setContextOpen] = useState(false);
  const [proposals, setProposals] = useState<LensProposal[]>([]);
  const [proposalsLoaded, setProposalsLoaded] = useState(false);
  const [dismissing, setDismissing] = useState<string | null>(null);
  const dismissPending = useRef(false);
  const patchView = (view: KnowledgeLocation["view"]) => onPatch?.({ ...location, view: { ...location.view, ...view } });
  const openLink = onOpenLink ?? ((url: string) => bridge.openExternalUrl(url));

  useEffect(() => {
    const panel = workspaceRef.current;
    if (!panel) return;
    const measure = (width: number) => { if (width > 0) { setCompact(width <= 760); setWide(width >= 1040); } };
    measure(panel.getBoundingClientRect().width);
    const observer = new ResizeObserver((entries) => { if (entries[0]) measure(entries[0].contentRect.width); });
    observer.observe(panel);
    return () => observer.disconnect();
  }, []);
  const openLensLink = (targetId: string) => {
    const target = [...(server?.links ?? []), ...(server?.backlinks ?? []), ...catalog.lenses].find((item) => item.recordId === targetId);
    if (!target) { setError(t("lensWorkspace.couldNotLoadThisLens")); return; }
    setContextOpen(false);
    onOpen({ loupistId: location.loupistId, surface: "lens", recordId: target.recordId, objectId: target.recordId, view: { ...location.view, readerScrollTop: 0, reviewProposal: undefined }, selection: { kind: "lens", recordId: target.recordId } });
  };

  useEffect(() => {
    const ticket = ++generation.current;
    basisGeneration.current++;
    setProposals([]); setProposalsLoaded(false); setDismissing(null); dismissPending.current = false;
    setServer(null); setError(null); setLoadError(null); setBasis(null); setLoadingBasis(false); setBasisError(null);
    if (!recordId) return;
    void bridge.knowledgeLensRead({ loupistId: location.loupistId, recordId, revisionId: location.revisionId }).then((view) => {
      if (ticket !== generation.current) return;
      setServer(view);
    }).catch((cause: unknown) => { if (ticket === generation.current) setLoadError(errorMessage(cause, t("lensWorkspace.couldNotLoadThisLens"))); });
    void bridge.knowledgeLensProposals({ loupistId: location.loupistId, recordId }).then((next) => {
      if (ticket === generation.current) { setProposals(next); setProposalsLoaded(true); }
    }).catch((cause: unknown) => { if (ticket === generation.current) setLoadError(errorMessage(cause, t("lensWorkspace.couldNotLoadLensProposals"))); });
    return () => { generation.current++; basisGeneration.current++; };
  }, [bridge, scope, reload]);

  useEffect(() => {
    if (!server) return;
    if (!markdownFirst || document.activeElement === document.body || workspaceRef.current?.contains(document.activeElement)) {
      readerRef.current?.focus({ preventScroll: true });
    }
    if (readerScrollRef.current) readerScrollRef.current.scrollTop = location.view?.readerScrollTop ?? 0;
  }, [server?.recordId, server?.revisionId]);
  const perspective = (server?.lensKind ?? record?.lensKind) === "perspective";
  const loadBasis = async () => {
    if (!recordId || loadingBasis) return;
    const ticket = ++basisGeneration.current;
    const captured = scope;
    setLoadingBasis(true);
    setBasisError(null);
    try {
      const records = await bridge.knowledgeLensChanges({ loupistId: location.loupistId, recordId });
      if (scopeRef.current === captured && basisGeneration.current === ticket) setBasis(records);
    } catch (cause) { if (scopeRef.current === captured && basisGeneration.current === ticket) setBasisError(errorMessage(cause, t("lensWorkspace.couldNotOpenTheSavedBasis"))); }
    finally { if (scopeRef.current === captured && basisGeneration.current === ticket) setLoadingBasis(false); }
  };
  const displayTitle = server?.isStanding && server.title === HOST_STANDING_TITLE ? t("lensWorkspace.defaultPerspective") : server?.title;
  const emptyPerspective = server?.isStanding && isStandingPlaceholder(server.document.markdown);
  const shownDocument = emptyPerspective ? { ...server.document, markdown: "" } : server?.document;
  const shownMarkdown = shownDocument ? bodyWithoutRepeatedTitle(server?.title ?? "", shownDocument.markdown) : "";
  const headings = useMemo(() => markdownOutline(shownMarkdown).map((item) => ({ ...item, depth: item.depth + 1 })), [shownMarkdown]);
  const mainOutline = [
    ...(shownMarkdown.trim() ? [{ key: "markdown", title: perspective ? t("lensWorkspace.perspective") : t("lensWorkspace.method"), depth: 1 }] : []),
    ...headings,
  ];
  const outline: LensOutlineItem[] = shownDocument ? [
    ...fields().filter(([key]) => (key === "applicability" || key === "limitations") && shownDocument[key].trim()).map(([key, title]) => ({ key, title, depth: 1 })),
    ...mainOutline,
    ...fields().filter(([key]) => key !== "applicability" && key !== "limitations" && shownDocument[key].trim()).map(([key, title]) => ({ key, title, depth: 1 })),
  ] : [];
  const jumpTo = (item: LensOutlineItem) => {
    setContextOpen(false);
    setShowMarkdown(false);
    requestAnimationFrame(() => {
      const panel = readerScrollRef.current?.querySelector(".lens-saved-content");
      const target = item.headingIndex !== undefined ? panel?.querySelectorAll<HTMLElement>(".lens-main-content h1, .lens-main-content h2, .lens-main-content h3, .lens-main-content h4, .lens-main-content h5, .lens-main-content h6")[item.headingIndex]
        : panel?.querySelector<HTMLElement>(`[data-lens-section="${item.key}"]`);
      if (target) { target.tabIndex = -1; target.focus({ preventScroll: true }); target.scrollIntoView({ block: "start" }); }
    });
  };
  const lensLinks: LensLinkActions = {
    state: (id) => {
      const item = catalog.lenses.find((lens) => lens.recordId === id);
      return item ? item.archived ? "archived" : "available" : "unresolved";
    },
    preview: (id) => {
      const item = catalog.lenses.find((lens) => lens.recordId === id);
      return item ? { title: item.title, detail: item.applicability || item.summary } : null;
    },
    onUnresolved: openLensLink,
  };
  const contextPane = server ? <>
    <LensContextPane outline={outline} view={server} onJump={jumpTo} onOpen={openLensLink} />
  </> : null;

  const request = location.view?.reviewProposal;
  const requestedProposal = request && recordId ? matchLensProposal(proposals, { recordId, ...request }) : undefined;
  const bannerProposals = request ? (requestedProposal ? [requestedProposal] : []) : proposals;
  const dismissProposal = async (proposal: LensProposal) => {
    if (dismissPending.current) return;
    const ticket = generation.current;
    dismissPending.current = true; setDismissing(proposal.draftId); setError(null);
    try {
      await bridge.knowledgeLensProposalDismiss({ loupistId: location.loupistId, draftId: proposal.draftId });
      if (!mounted.current || ticket !== generation.current) return;
      setProposals((current) => current.filter((item) => item.draftId !== proposal.draftId));
      if (request) patchView({ reviewProposal: undefined });
      onChanged();
    } catch (cause) {
      if (mounted.current && ticket === generation.current) setError(errorMessage(cause, t("lensWorkspace.couldNotDismissThisProposal")));
    } finally {
      if (ticket === generation.current) { dismissPending.current = false; setDismissing(null); }
    }
  };
  const principle = (record?.lensKind ?? server?.lensKind) === "principle";
  const sections: [string, string][] = !server ? [] : principle
    ? [[t("principles.why"), server.document.applicability], [t("principles.example"), server.document.procedure]]
    : fields().map(([key, label]) => [label, server.document[key]]);
  const documentActions = server ? <div className="lens-reading-tools"><Button variant="outline" onClick={() => onAsk(pinForLens(location.loupistId, server))}><MessageSquareIcon />{t("lensWorkspace.discussInChat")}</Button>
    {!perspective ? <Button variant="outline" size="sm" data-share-item onClick={() => setShareOpen(true)}>{i18n.t("sharing:share")}</Button> : null}</div> : null;
  return <div ref={workspaceRef} className={`knowledge-lens lens-workspace is-reader-only${compact ? " is-compact" : ""}${wide ? " has-context-pane" : ""}`}>
    <div ref={readerScrollRef} className="knowledge-reading lens-reader" onScroll={event => patchView({ readerScrollTop: event.currentTarget.scrollTop })}>
      {!wide ? <div className="lens-reader-toolbar"><Button variant="ghost" aria-label={t("lensWorkspace.showOutlineAndLinks")} onClick={() => setContextOpen(true)} disabled={!server}><ListTreeIcon />{t("lensWorkspace.outlineLinks")}</Button></div> : null}
      {loadError ? <Alert variant="destructive"><AlertDescription>{loadError}</AlertDescription><Button variant="outline" onClick={() => setReload((value) => value + 1)}>{t("lensWorkspace.retryLens")}</Button></Alert> : null}
      {error ? <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert> : null}
      {request && proposalsLoaded && !requestedProposal ? <Alert variant="destructive"><AlertDescription>{proposalMatchError()}</AlertDescription></Alert> : null}
      {server && shownDocument ? <article className="lens-document" aria-label={t("lensWorkspace.lensReader")}>
        <header className="knowledge-read-head lens-read-head"><div><div className="lens-document-meta"><span className="lens-eyebrow">{t("map.lensWiki")}</span><span>{perspective ? t("lensWorkspace.defaultPerspective") : principle ? t("map.kind.principle") : t("lensWorkspace.researchMethod")}</span></div><h2 ref={readerRef} tabIndex={-1}>{displayTitle}</h2>
          <SharedOrigin bridge={bridge} loupistId={location.loupistId} subjectId={server.recordId} onAbout={() => onOpen({ loupistId: location.loupistId, surface: "lens", recordId: SHARED_SECTION_ITEM })} /></div></header>
        {markdownFirst ? <div className="lens-format-picker">
          <Button variant="ghost" size="sm" aria-pressed={showMarkdown} onClick={() => setShowMarkdown(true)}>Markdown</Button>
          <Button variant="ghost" size="sm" aria-pressed={!showMarkdown} onClick={() => setShowMarkdown(false)}>{t("lensWorkspace.formatted")}</Button>
        </div> : <LensMarkdownSource key={`markdown:${scope}`} markdown={server.document.markdown} sections={sections} />}
        {showMarkdown && <div className="lens-markdown-source"><pre>{markdownText(shownDocument.markdown, sections)}</pre></div>}
        {emptyPerspective && <p className="knowledge-hint">{t("lensWorkspace.emptyPerspective")}</p>}
        {!server.readOnly ? bannerProposals.map((proposal, index) => <details className="lens-proposal" key={proposal.draftId} open={request ? true : undefined}>
          <summary>{proposal.originChatTitle ? t("lensWorkspace.proposalFromChat", { title: proposal.originChatTitle }) : t("lensWorkspace.proposalNumber", { index: formatNumber(index + 1) })}</summary>
          <section aria-label={t("lensWorkspace.proposalPosition", { index: formatNumber(index + 1), total: formatNumber(bannerProposals.length) })}>
            <p>{proposal.rationale}</p>
            <p className="knowledge-hint">{t("lensWorkspace.askYourLoupistInChatToReviseOrApplyThis")}</p>
            <LensDocumentContent title={proposal.title ?? server.title} document={proposal.document} onOpenLink={openLink} onLensLink={openLensLink} lensLinks={lensLinks} />
            <div className="knowledge-inspector-actions"><Button variant="outline" onClick={() => onAsk(pinForLens(location.loupistId, server), proposal.originChatId)}><MessageSquareIcon />{t("lensWorkspace.discussProposalInChat")}</Button><Button variant="ghost" disabled={dismissing !== null} onClick={() => void dismissProposal(proposal)}>{dismissing === proposal.draftId ? t("lensWorkspace.dismissing") : t("lensWorkspace.dismiss")}</Button></div>
          </section>
        </details>) : null}
        {showMarkdown ? documentActions : <div className="lens-saved-content"><LensDocumentContent title={server.title} document={shownDocument} onOpenLink={openLink} onLensLink={openLensLink} lensLinks={lensLinks}>{documentActions}</LensDocumentContent></div>}
        {children}
        <details key={scope} className="lens-basis" onToggle={(event) => { if (event.currentTarget.open && !basis) void loadBasis(); }}>
          <summary>{t("lensWorkspace.savedBasis")}</summary>
          {loadingBasis ? <p role="status" className="memory-loading-inline"><Spinner />{t("lensWorkspace.openingSavedBasis")}</p> : null}
          {basisError ? <Alert variant="destructive"><AlertDescription>{basisError}</AlertDescription><Button variant="outline" disabled={loadingBasis} onClick={() => void loadBasis()}>{t("lensWorkspace.retrySavedBasis")}</Button></Alert> : null}
          {basis?.length === 0 ? <p className="knowledge-hint">{t("lensWorkspace.noSavedBasis")}</p> : null}
          {basis?.slice(0, 1).map((item) => <section key={item.changeId}>
            <p>{item.rationale}</p>
            {item.inputs.references.length ? <ul aria-label={t("lensWorkspace.savedEvidence")}>{item.inputs.references.map((reference, index) => <li key={`${reference.kind}:${reference.recordId}:${reference.revisionId}:${reference.objectId}:${reference.spanId ?? ""}`}><Button variant="link" onClick={() => onOpen(knowledgeLocationFromReference(location.loupistId, reference))}>{evidenceLabel(reference.kind, index)}</Button></li>)}</ul> : null}
          </section>)}
        </details>
      </article> : recordId && !error && !loadError ? <p role="status" className="memory-loading-inline"><Spinner />{t("lensWorkspace.openingLens")}</p> : !error && !loadError ? <EmptyState title={t("lensWorkspace.noLensSelected")} description={t("lensWorkspace.chooseAPerspectiveOrMethodAskYourLoupistInChat")} /> : null}
    </div>
    {wide ? <aside className="lens-context-pane" aria-label={t("lensWorkspace.lensContext")}>{contextPane}</aside> : <Dialog open={contextOpen} onOpenChange={setContextOpen}><DialogContent size="sm" className="lens-context-dialog"><DialogHeader><DialogTitle>{t("lensWorkspace.outlineLinks")}</DialogTitle><DialogDescription>{displayTitle}</DialogDescription></DialogHeader><DialogBody>{contextPane}</DialogBody></DialogContent></Dialog>}
    {server ? <ShareSheet bridge={bridge} loupistId={location.loupistId} loupistName="" scope={{ kind: "item", recordId: server.recordId }} open={shareOpen} onOpenChange={setShareOpen} /> : null}
  </div>;
}
