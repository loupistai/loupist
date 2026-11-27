import { Fragment, useContext, useEffect, useState, useRef } from "react";
import { XIcon } from "lucide-react";
import type { LoupistBridge, KnowledgeLocation, KnowledgePin } from "../../types";
import { useTranslation } from "../../i18n";
import { pinForClaim, sourceTargetLocation } from "../../knowledge/location";
import type { GraphClaim } from "../../contracts/knowledge";
import { claimCorrectionDraft, claimWords, learnedLine, publishedLine, validityLine } from "./claimFacts";
import { historyEpisode } from "./claimHistory";
import type { ClaimHistory } from "../../contracts/knowledge";
import { dateValue } from "../../knowledge/presentation";
import { formatNumber } from "../../i18n/format";
import { Button } from "../ui/button";
import { useInspectorScroll, useMapDetail, WorldAsOfContext } from "./useMapData";
/** A selected claim and its exact retained evidence in the current World. */
export function MapClaimInspector({ bridge, location, claimId, onClose, onOpen, onAsk, onCorrect, onRevisionChange }: {
  bridge: LoupistBridge; location: KnowledgeLocation; claimId: string; onRevisionChange?: () => void; onClose: () => void;
  onOpen: (location: KnowledgeLocation) => void; onAsk: (pin: KnowledgePin) => void;
  /** Puts a correction of this claim into a research session's composer; never sends. */
  onCorrect?: (text: string) => void;
}) {
  const { t } = useTranslation("knowledge");
  const { graph, error, retry } = useMapDetail(bridge, location, claimId, "claim", onRevisionChange);
  const scroll = useInspectorScroll(claimId, !!graph);
  const claim = graph?.edges.find((e) => e.claim.id === claimId || e.id === claimId)?.claim;
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus({ preventScroll: true }); }, [claimId]);
  return <aside className="map-inspector" {...scroll} aria-labelledby="map-claim-title"><Button className="map-inspector-close" variant="ghost" size="icon-sm" aria-label={t("map.close")} onClick={onClose}><XIcon /></Button>
    <h2 id="map-claim-title" ref={heading} tabIndex={-1}>{claim?.proposition ?? t("map.keyFacts")}</h2>
    {!graph && !error ? <p role="status">{t("map.loadingFacts")}</p> : null}
    {error ? <p role="alert">{t("map.detailError")}<Button onClick={retry}>{t("map.retry")}</Button></p> : null}
    {claim ? <><ClaimFacts claim={claim} /><ClaimHistoryPanel bridge={bridge} location={location} claimId={claim.id} onOpen={onOpen} />{claim.evidence.length ? <ul className="map-facts">{claim.evidence.map((e, index) => {
      const target = e.spanId ? graph?.sourceTargets?.[e.spanId] : undefined;
      return <li key={`${e.spanId}:${index}`}><p>{e.attribution}</p>
        {e.figureCheck === "not_found" ? <p className="map-figure-check" data-figure-check="not_found">{t("map.figureNotFound")}</p> : null}
        {target ? <Button variant="link" onClick={() => onOpen(sourceTargetLocation(location.loupistId, target, e.passage))}>{t("map.openSource")}</Button> : null}</li>;
    })}</ul> : null}<footer><Button onClick={() => onAsk(pinForClaim(location.loupistId, graph!.recordId, graph!.revisionId, claim))}>{t("map.askAbout")}</Button>{onCorrect ? <Button variant="outline" data-claim-correct onClick={() => onCorrect(claimCorrectionDraft(claim, t, Date.now()))}>{t("map.correction.action")}</Button> : null}</footer></> : graph ? <p>{t("map.noClaimFacts")}</p> : null}
  </aside>;
}

export function ClaimHistoryPanel({ bridge, location, claimId, onOpen }: {
  bridge: LoupistBridge; location: KnowledgeLocation; claimId: string; onOpen: (location: KnowledgeLocation) => void;
}) {
  const { t } = useTranslation("knowledge");
  const [open, setOpen] = useState(false);
  const [history, setHistory] = useState<ClaimHistory | "loading" | "error" | null>(null);
  const asOf = useContext(WorldAsOfContext);
  const [more, setMore] = useState(false);
  useEffect(() => {
    if (!open) return;
    let live = true;
    setHistory("loading");
    void bridge.knowledgeWorldClaimHistory({ loupistId: location.loupistId, claimId, ...(asOf ? { asOf } : {}) })
      .then(next => { if (live) setHistory(next); })
      .catch(() => { if (live) setHistory("error"); });
    return () => { live = false; };
  }, [open, bridge, location.loupistId, claimId, asOf]);
  const openSource = (episode: ReturnType<typeof historyEpisode>) => {
    if (!episode?.recordId) return;
    onOpen({ loupistId: location.loupistId, surface: "source", recordId: episode.recordId, revisionId: episode.revisionId, selection: { kind: "source", spanId: episode.spanId, sourceId: episode.recordId } });
  };
  const openClaim = (id: string) => onOpen({ ...location, surface: "world", selection: { kind: "claim", id } });
  const line = (value: unknown) => {
    const episode = historyEpisode(value);
    if (!episode) return null;
    const date = episode.date ? dateValue(episode.date) : "";
    return <li>{episode.recordId ? <Button type="button" variant="link" onClick={() => openSource(episode)}>{episode.title || t("map.openSource")}</Button> : <span>{episode.title}</span>}{date ? ` · ${date}` : ""}</li>;
  };
  const supported = history && history !== "loading" && history !== "error" ? history.supported : [];
  const shown = more ? supported : supported.slice(0, 5);
  return <details className="map-claim-history" onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>{t("map.historyTitle")}</summary>
    {open && history === "loading" ? <p role="status">{t("map.historyLoading")}</p> : null}
    {history === "error" ? <p role="alert">{t("map.historyError")}</p> : null}
    {history && history !== "loading" && history !== "error" ? <>
      <h3>{t("map.historyCreated")}</h3>
      <ul>{line(history.created)}</ul>
      <h3>{t("map.historySupported")}</h3>
      <ul>{shown.map((item, index) => <Fragment key={index}>{line(item)}</Fragment>)}</ul>
      {supported.length > 5 && !more ? <Button type="button" variant="link" onClick={() => setMore(true)}>{t("map.historyMore", { count: supported.length - 5, formattedCount: formatNumber(supported.length - 5) })}</Button> : null}
      <h3>{t("map.historyEnded")}</h3>
      <ul>{line(history.ended)}</ul>
      {history.endedByClaimId ? <Button type="button" variant="link" onClick={() => openClaim(history.endedByClaimId!)}>{t("map.historyOpenEnded")}</Button> : null}
      <h3>{t("map.historyEndedThese")}</h3>
      <ul>{history.endedClaims.map(id => <li key={id}><Button type="button" variant="link" onClick={() => openClaim(id)}>{t("map.historyOpenClaim")}</Button></li>)}</ul>
    </> : null}
  </details>;
}

function ClaimFacts({ claim }: { claim: GraphClaim }) {
  const { t } = useTranslation("knowledge");
  const now = Date.now();
  const published = publishedLine(claim, t);
  const learned = learnedLine(claim, t);
  return <div className="map-claim-meta">
    <p>{t(`map.claimStatus.${claimWords(claim, now)}`)}</p>
    <p>{validityLine(claim, t, now)}</p>
    {published ? <p>{published}</p> : null}
    {learned ? <p>{learned}</p> : null}
  </div>;
}
