import { relationName } from "./relations";
import { ResearchReader } from "../knowledge/ResearchReader";
import { ConnectionOrigin } from "../connections/custom/ConnectionOrigin";
import { useEffect, useRef, useState } from "react";
import { ArrowLeftIcon, XIcon } from "lucide-react";
import { LensLearningPanel } from "../knowledge/LensLearningPanel";
import type { KnowledgeMap, KnowledgeMapNode } from "../../contracts/knowledge";
import type { LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin, PageSummary, WorldGraphView } from "../../types";
import { useTranslation } from "../../i18n";
import { formatDate } from "../../i18n/format";
import { PrincipleForm } from "./PrincipleForm";
import type { Principle } from "../../types";
import { dateValue } from "../../knowledge/presentation";
import { pinForClaim, pinForNode } from "../../knowledge/location";
import { Button } from "../ui/button";
import { extensionKindLabel, nodeCategory } from "./kinds";
import { entityLabel } from "./labels";
import { useMapDetail } from "./useMapData";
import { MapRelationships } from "./MapRelationships";
import { hasRelationshipsView } from "./relationships";
import { isBasePerspective, isLensNode, isStandingPlaceholder, lensDisplayTitle } from "./lensReading";
import { InspectorThesis } from "../assets/InspectorThesis";
import { sharingReleaseVisible } from "../../sharing/releaseVisibility";
import { SharedOrigin } from "../sharing/SharedOrigin";
import { SHARED_SECTION_ITEM } from "../sharing/SharedPacks";
export function MapInspector({ node, map, bridge, location, onClose, onSelect, onOpen, onAsk, providedGraph, catalog, onChanged, onOpenChat, onExplainDisagreement, onPointRelationship }: {
  catalog?: KnowledgeCatalog; onChanged?: () => void; onOpenChat?: (chatId: string) => void;
  /** A relationship chip the user points at; the canvas widens that line. */
  onPointRelationship?: (edgeId?: string) => void;
  /** Opens a new topic Chat with the entity and relation chips and the question filled; never sends. */
  onExplainDisagreement?: (pins: KnowledgePin[]) => void;
  providedGraph?: WorldGraphView;
  node: KnowledgeMapNode; map: KnowledgeMap; bridge: LoupistBridge; location: KnowledgeLocation;
  onClose: () => void; onSelect: (node: KnowledgeMapNode) => void; onOpen: (location: KnowledgeLocation) => void; onAsk: (pin: KnowledgePin) => void;
}) {
  const { t, i18n } = useTranslation("knowledge");
  const name = entityLabel(node, i18n.language);
  const heading = useRef<HTMLHeadingElement>(null);
  const [more, setMore] = useState(false);
  const [allConnections, setAllConnections] = useState(false);
  const detail = useMapDetail(bridge, location, node.kind === "entity" && !providedGraph ? node.id : undefined);
  const graph = providedGraph ?? detail.graph;
  const { error, retry } = detail;
  const catalogLenses = catalog?.lenses ?? [];
  const lens = catalogLenses.find(l => l.recordId === node.recordId);
  const basePerspective = isBasePerspective(node, catalogLenses);
  const headingText = isLensNode(node) ? lensDisplayTitle(node, catalogLenses) : name;
  const entity = graph?.nodes.find((n) => n.id === node.id);
  const [notes, setNotes] = useState<PageSummary[]>([]);
  const noteScope = node.kind === "entity" ? node.id : undefined;
  useEffect(() => {
    setNotes([]);
    if (!noteScope) return;
    let live = true;
    void bridge.knowledgeLensPagesForNode({ loupistId: location.loupistId, nodeId: noteScope }).then(found => { if (live) setNotes(found); }).catch(() => {});
    return () => { live = false; };
  }, [bridge, location.loupistId, noteScope, catalog]);
  // Facts or Relationships; the choice stays while the user walks from name to name.
  const [view, setView] = useState<"facts" | "relationships">("facts");
  const relationshipsAvailable = hasRelationshipsView(node);
  const showRelationships = relationshipsAvailable && view === "relationships";
  // Walking from a relationship chip keeps a back stack; selecting from anywhere else starts over.
  const [trail, setTrail] = useState<KnowledgeMapNode[]>([]);
  const walkingTo = useRef<string | null>(null);
  useEffect(() => {
    if (walkingTo.current !== node.id) setTrail([]);
    walkingTo.current = null;
  }, [node.id]);
  const walk = (next: KnowledgeMapNode) => {
    if (next.id === node.id) return;
    if (!isLensNode(next)) {
      setTrail((current) => [...current, node].slice(-20));
      walkingTo.current = next.id;
    }
    onSelect(next);
  };
  const walkBack = () => {
    const previous = trail.at(-1);
    if (!previous) return;
    setTrail((current) => current.slice(0, -1));
    walkingTo.current = previous.id;
    onSelect(previous);
  };
  useEffect(() => { heading.current?.focus(); setMore(false); setAllConnections(false); }, [node.id]);
  const claims = [...(graph?.edges ?? [])].filter((e) => e.endpoints.some((p) => p.nodeId === node.id)).sort((a, b) => (b.claim.recordedAtMs ?? 0) - (a.claim.recordedAtMs ?? 0)).slice(0, 6);
  const disputed = (graph?.edges ?? []).filter(edge => edge.claim.status === "disputed" && edge.endpoints.some(endpoint => endpoint.nodeId === node.id));
  // The World contract has no pair ID. Group reports about the same relation and
  // role-labelled endpoints, preserving every attributed report in the group.
  const conflictGroups = [...new Map(disputed.map(edge => {
    const key = (item: typeof edge) => JSON.stringify([item.relation.toUpperCase(), item.endpoints.map(endpoint => `${endpoint.role}:${endpoint.nodeId}`).sort()]);
    return [key(edge), (graph?.edges ?? []).filter(other => key(other) === key(edge) && other.claim.status !== "withdrawn")];
  })).values()];
  const connected = map.edges.filter((e) => e.source === node.id || e.target === node.id);
  const lenses = connected.filter((e) => e.kind === "applies_to").map((e) => map.nodes.find((n) => n.id === (e.source === node.id ? e.target : e.source))).filter((n): n is KnowledgeMapNode => !!n && n.kind !== "entity");
  const connections = connected.filter((e) => e.kind !== "applies_to").sort((a, b) => (a.relation ?? "").localeCompare(b.relation ?? ""));
  const openSource = (spanId: string) => {
    const target = graph?.sourceTargets?.[spanId];
    if (target) onOpen({ loupistId: location.loupistId, surface: "source", recordId: target.recordId, revisionId: target.revisionId, objectId: target.sourceId, selection: { kind: "source", sourceId: target.sourceId, spanId: target.spanId, worldRecordId: target.worldRecordId, worldRevisionId: target.worldRevisionId } });
  };
  if (node.kind === "journal") return <aside className="map-inspector journal-inspector" aria-label={t("journal.entry")}><Button variant="ghost" size="icon-sm" className="map-inspector-close" aria-label={t("map.close")} onClick={onClose}><XIcon /></Button><ResearchReader bridge={bridge} location={{ loupistId: location.loupistId, surface: "note", selection: { kind: "journal", journalId: node.id } }} onOpen={onOpen} onAsk={onAsk} onOpenChat={onOpenChat} onChanged={onChanged} /></aside>;
  if (node.kind === "lens_principle" && node.recordId) return <PrincipleInspector key={`${location.loupistId}:${node.recordId}`} bridge={bridge} loupistId={location.loupistId} refreshKey={lens?.revisionId ?? node.label} node={node} map={map} onClose={onClose} onSelect={onSelect} onChanged={onChanged} onOpenChat={onOpenChat} />;
  return <aside className="map-inspector" aria-labelledby="map-inspector-title"><Button variant="ghost" size="icon-sm" className="map-inspector-close" aria-label={t("map.close")} onClick={onClose}><XIcon /></Button>
    {trail.length ? <Button variant="ghost" size="sm" className="map-inspector-back" onClick={walkBack}><ArrowLeftIcon />{t("map.back")}</Button> : null}
    <div className="map-kind-chip"><i className={`map-swatch is-${nodeCategory(node)}`} />{extensionKindLabel(node.kindLabels, i18n.language, t(`map.kind.${nodeCategory(node)}`))}{node.watched ? <span>{t("map.kind.watching")}</span> : null}{node.held ? <span>{t("map.kind.holding")}</span> : null}</div>
    <h2 id="map-inspector-title" tabIndex={-1} ref={heading}>{headingText}</h2>
    {!isLensNode(node) && name !== node.label ? <p className="map-muted">{node.label}</p> : null}
    {basePerspective ? <p className="map-base-perspective">{t("principles.perspectiveCaption")}</p> : null}
    <ConnectionOrigin origin={node.origin} />
    <SharedOrigin bridge={bridge} loupistId={location.loupistId} subjectId={node.recordId ?? node.id} onAbout={sharingReleaseVisible() ? () => onOpen({ loupistId: location.loupistId, surface: "lens", recordId: SHARED_SECTION_ITEM }) : undefined} />
    {node.kind !== "entity" && node.recordId ? <LensLearningPanel key={`${location.loupistId}:${node.recordId}:${lens?.revisionId ?? ""}`} bridge={bridge} loupistId={location.loupistId} node={node} onArchived={() => { onChanged?.(); onClose(); }} /> : null}
    {node.kind !== "entity" && lens?.summary && !isStandingPlaceholder(lens.summary) ? <p className="map-lens-applicability">{lens.summary}</p> : null}
    {entity?.aliases.length ? <p className="map-aliases" title={entity.aliases.join(", ")}>{entity.aliases.join(", ")}</p> : null}
    {entity?.description ? <><p className={more ? "" : "map-summary"}>{entity.description}</p><Button variant="link" onClick={() => setMore(!more)}>{t(more ? "map.less" : "map.more")}</Button></> : null}
    {error ? <p role="alert">{t("map.detailError")} <Button variant="link" onClick={retry}>{t("map.retry")}</Button></p> : null}
    {conflictGroups.length > 0 && <section className="map-conflicts" aria-label={t("map.conflictingEvidence")}><h3>{t("map.conflictingEvidence")}</h3>{conflictGroups.map(group => <div className="map-conflict-pair" key={group[0].id}>{group.map(({ claim }) => <div key={claim.id}><p>{claim.proposition}</p>{claim.evidence.length ? claim.evidence.map((evidence, index) => {
      const target = graph?.sourceTargets?.[evidence.spanId];
      const source = catalog?.sources.flatMap(record => record.objects).find(source => source.id === target?.sourceId);
      const date = source?.publicationTime?.value ?? claim.publicationTime?.value ?? (claim.recordedAtMs ? new Date(claim.recordedAtMs).toISOString() : undefined);
      return <small key={`${evidence.spanId}:${index}`}>{source?.title ?? evidence.attribution ?? claim.attribution}{date ? ` · ${dateValue(date)}` : ""}{target && <Button variant="link" onClick={() => openSource(evidence.spanId)}>{t("map.openSource")}</Button>}</small>;
    }) : <small>{claim.attribution}{claim.publicationTime?.value ? ` · ${dateValue(claim.publicationTime.value)}` : ""}</small>}</div>)}</div>)}
      {onExplainDisagreement && location.recordId && location.revisionId ? <Button variant="ghost" size="sm" data-explain-disagreement aria-label={`${t("map.explainDisagreement")} · ${name}`} onClick={() => {
        const recordId = location.recordId!;
        const revisionId = location.revisionId!;
        const subject = pinForNode(location.loupistId, recordId, revisionId, entity ?? { id: node.id, name: node.label, aliases: [], description: "", kind: node.entityKind ?? "", sourceRole: "", properties: {} });
        // One relation chip per disputed group: the reports the question is about.
        onExplainDisagreement([subject, ...conflictGroups.slice(0, 3).map(group => pinForClaim(location.loupistId, recordId, revisionId, (group.find(edge => edge.claim.status === "disputed") ?? group[0]!).claim))]);
      }}>{t("map.explainDisagreement")}</Button> : null}</section>}
    {relationshipsAvailable ? <div className="map-inspector-views map-segmented" role="group" aria-label={t("map.relationshipsView.views")}>
      {(["facts", "relationships"] as const).map((id) => <button key={id} type="button" aria-pressed={view === id} onClick={() => setView(id)}>{t(`map.relationshipsView.${id}`)}</button>)}
    </div> : null}
    {showRelationships ? <MapRelationships map={map} node={node} onSelect={(next) => { onPointRelationship?.(); walk(next); }} onPoint={onPointRelationship} /> : null}
    {showRelationships ? null : node.kind === "entity" ? <><InspectorThesis bridge={bridge} loupistId={location.loupistId} entityId={node.id} /><h3>{t("map.keyFacts")}</h3>{!graph && !error ? <p role="status">{t("map.loadingFacts")}</p> : null}
      <ul className="map-facts">{claims.map(({ claim }) => <li key={claim.id}><p>{claim.proposition}</p><small>{claim.publicationTime?.value ? dateValue(claim.publicationTime.value) : claim.recordedAtMs ? dateValue(new Date(claim.recordedAtMs).toISOString()) : null}{[...new Set(claim.evidence.map((e) => e.spanId).filter((id): id is string => !!id && !!graph?.sourceTargets?.[id]))].map((spanId) => <Button variant="link" key={spanId} onClick={() => openSource(spanId)}>{t("map.openSource")}</Button>)}</small></li>)}</ul>
      {graph && !claims.length ? <p className="map-muted">{t("map.noFacts")}</p> : null}
      {notes.length ? <section className="map-notes" aria-label={t("map.notes")}><h3>{t("map.notes")}</h3>
        <ul>{notes.map((note) => <li key={note.recordId}><button type="button" onClick={() => onOpen({ loupistId: location.loupistId, surface: "lens", recordId: note.recordId, objectId: note.recordId, selection: { kind: "lens", recordId: note.recordId } })}>
          <strong>{note.title}</strong><small>{t(`pageReader.kind.${note.kind}`)}</small>{note.summary ? <span>{note.summary}</span> : null}</button></li>)}</ul></section> : null}
      <h3>{t("map.seenThrough")}</h3>{lenses.map((item) => <Button variant="ghost" className="map-connection" key={item.id} onClick={() => onSelect(item)}><i className={`map-swatch is-${nodeCategory(item)}`} />{lensDisplayTitle(item, catalogLenses)}</Button>)}{!lenses.length ? <p className="map-muted">{t("map.noLens")}</p> : null}
    </> : <><h3>{connected.filter((e) => e.kind === "applies_to").length ? t("map.appliedTo", { count: connected.filter((e) => e.kind === "applies_to").length }) : t("map.notApplied")}</h3>{connected.filter((e) => e.kind === "applies_to").map((e) => { const target = map.nodes.find((n) => n.id === e.target); return target ? <Button className="map-connection" variant="ghost" key={e.id} onClick={() => onSelect(target)}>{entityLabel(target, i18n.language)}</Button> : null; })}{node.recordId ? <Button variant="outline" onClick={() => onOpen({ loupistId: location.loupistId, surface: "lens", recordId: node.recordId })}>{t("map.openDocument")}</Button> : null}</>}
    {/* The Relationships view already shows every connection with its direction; the flat list belongs to Facts. */}
    {showRelationships ? null : <><h3>{t("map.connections", { count: connections.length })}</h3>{(allConnections ? connections : connections.slice(0, 8)).map((edge, index, rows) => { const neighbor = map.nodes.find((n) => n.id === (edge.source === node.id ? edge.target : edge.source)); return neighbor ? <div key={edge.id}>{index === 0 || rows[index - 1].relation !== edge.relation ? <small className="map-relation">{(edge.relation ? relationName(edge.relation, t) : undefined)}</small> : null}<Button variant="ghost" className="map-connection" onClick={() => onSelect(neighbor)}><i className={`map-swatch is-${nodeCategory(neighbor)}`} />{entityLabel(neighbor, i18n.language)}</Button></div> : null; })}
    {connections.length > 8 ? <Button variant="link" onClick={() => setAllConnections(!allConnections)}>{t(allConnections ? "map.less" : "map.more")}</Button> : null}</>}
    <footer>{node.kind === "entity" && location.recordId && location.revisionId ? <Button onClick={() => onAsk(pinForNode(location.loupistId, location.recordId!, location.revisionId!, entity ?? { id: node.id, name: node.label, aliases: [], description: "", kind: node.entityKind ?? "", sourceRole: "", properties: {} }))}>{t("map.askAbout")}</Button> : null}
    <p className="map-disclosure">{t("map.disclosure")}</p></footer>
  </aside>;
}

function PrincipleInspector({ bridge, loupistId, refreshKey, node, map, onClose, onSelect, onChanged, onOpenChat }: {
  bridge: LoupistBridge; loupistId: string; refreshKey?: string; node: KnowledgeMapNode; map: KnowledgeMap;
  onClose: () => void; onSelect: (node: KnowledgeMapNode) => void; onChanged?: () => void; onOpenChat?: (chatId: string) => void;
}) {
  const { t, i18n } = useTranslation("knowledge");
  const [principle, setPrinciple] = useState<Principle>();
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState(false);
  const [pending, setPending] = useState(false);
  const [reload, setReload] = useState(0);
  const title = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    let current = true; setError(false);
    void bridge.principleList({ loupistId, includeArchived: true }).then(items => {
      if (!current) return;
      const item = items.find(item => item.recordId === node.recordId);
      setPrinciple(item); if (!item) setError(true);
    }).catch(() => { if (current) setError(true); });
    return () => { current = false; };
  }, [bridge, loupistId, node.recordId, refreshKey, reload]);
  useEffect(() => { title.current?.focus(); }, [principle?.recordId]);
  const neighbors = map.edges.filter(edge => edge.kind === "applies_to" && (edge.source === node.id || edge.target === node.id)).map(edge => map.nodes.find(item => item.id === (edge.source === node.id ? edge.target : edge.source))).filter((item): item is KnowledgeMapNode => !!item);
  async function archive() {
    if (!principle || pending) return;
    setPending(true); setError(false);
    try { await bridge.principleArchive({ loupistId, recordId: principle.recordId, archived: !principle.archived }); onChanged?.(); onClose(); }
    catch { setError(true); }
    finally { setPending(false); }
  }
  return <aside className="map-inspector principle-inspector" aria-labelledby="principle-inspector-title">
    <Button variant="ghost" size="icon-sm" className="map-inspector-close" aria-label={t("map.close")} onClick={onClose}><XIcon /></Button>
    <div className="map-kind-chip">{t("principles.kind")}</div>
    <h2 id="principle-inspector-title" tabIndex={-1} ref={title}>{principle?.statement ?? node.label}</h2>
    {error ? <div role="alert">{t(principle ? "principles.error" : "principles.unavailable")}<Button variant="ghost" onClick={() => setReload(value => value + 1)}>{t("principles.retry")}</Button></div> : null}
    {!principle && !error ? <p role="status">{t("principles.loading")}</p> : null}
    <LensLearningPanel key={`${loupistId}:${node.recordId}:${principle?.revisionId ?? refreshKey ?? ""}`} bridge={bridge} loupistId={loupistId} node={node} onArchived={() => { onChanged?.(); onClose(); }} />
    {principle ? <>
      {editing ? <PrincipleForm bridge={bridge} loupistId={loupistId} principle={principle} onCancel={() => setEditing(false)} onSaved={saved => { setPrinciple(saved); setEditing(false); onChanged?.(); }} /> : <>
        {principle.why ? <section><h3>{t("principles.why")}</h3><p>{principle.why}</p></section> : null}
        {principle.example ? <section><h3>{t("principles.example")}</h3><p>{principle.example}</p></section> : null}
      </>}
      <h3>{neighbors.length ? t("map.appliedTo", { count: neighbors.length }) : t("map.notApplied")}</h3>
      {neighbors.map(neighbor => <Button key={neighbor.id} className="map-connection" variant="ghost" onClick={() => onSelect(neighbor)}>{entityLabel(neighbor, i18n.language)}</Button>)}
      <p className="principle-origin">{t(principle.originChatId ? "principles.chatOrigin" : "principles.libraryOrigin", { date: formatDate(principle.createdAtMs) })}{principle.originChatId && onOpenChat ? <Button variant="link" onClick={() => onOpenChat(principle.originChatId!)}>{t("principles.openChat")}</Button> : null}</p>
      {!editing ? <div className="principle-actions"><Button variant="outline" onClick={() => setEditing(true)}>{t("principles.edit")}</Button><Button variant="ghost" disabled={pending} onClick={() => void archive()}>{t(principle.archived ? "principles.restore" : "principles.archive")}</Button></div> : null}
    </> : null}
  </aside>;
}
