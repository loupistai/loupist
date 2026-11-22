import { useEffect, useRef } from "react";
import { XIcon } from "lucide-react";
import type { LoupistBridge, KnowledgeLocation, KnowledgePin } from "../../types";
import { useTranslation } from "../../i18n";
import { pinForClaim } from "../../knowledge/location";
import { dateValue } from "../../knowledge/presentation";
import { Button } from "../ui/button";
import { useInspectorScroll, useMapDetail } from "./useMapData";
/** A selected claim and its exact retained evidence in the current World. */
export function MapClaimInspector({ bridge, location, claimId, onClose, onOpen, onAsk, onRevisionChange }: {
  bridge: LoupistBridge; location: KnowledgeLocation; claimId: string; onRevisionChange?: () => void; onClose: () => void;
  onOpen: (location: KnowledgeLocation) => void; onAsk: (pin: KnowledgePin) => void;
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
    {claim ? <><p>{claim.attribution}</p>{claim.publicationTime?.value ? <time>{dateValue(claim.publicationTime.value)}</time> : null}<ul className="map-facts">{claim.evidence.map((e, index) => {
      const target = e.spanId ? graph?.sourceTargets?.[e.spanId] : undefined;
      return <li key={`${e.spanId}:${index}`}><p>{e.attribution}</p>{target ? <Button variant="link" onClick={() => onOpen({ loupistId: location.loupistId, surface: "source", recordId: target.recordId, revisionId: target.revisionId, objectId: target.sourceId, selection: { kind: "source", sourceId: target.sourceId, spanId: target.spanId, worldRecordId: target.worldRecordId, worldRevisionId: target.worldRevisionId } })}>{t("map.openSource")}</Button> : null}</li>;
    })}</ul><footer><Button onClick={() => onAsk(pinForClaim(location.loupistId, graph!.recordId, graph!.revisionId, claim))}>{t("map.askAbout")}</Button></footer></> : graph ? <p>{t("map.noClaimFacts")}</p> : null}
  </aside>;
}
