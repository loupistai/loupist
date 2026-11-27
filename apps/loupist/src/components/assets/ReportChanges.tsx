import { useEffect, useState } from "react";
import type { LoupistBridge } from "../../types";
import type { WorldChangesView } from "../../contracts/knowledge";
import { useTranslation } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { dateValue } from "../../knowledge/presentation";
import { Button } from "../ui/button";
import { ChangeGroup } from "./ChangeRows";
import type { ChangeRow } from "./worldChanges";

type ChangeClaim = WorldChangesView["endedByTo"][number];
type ChangeSource = NonNullable<WorldChangesView["to"]>;

const point = (value: ChangeClaim["validAt"]): ChangeRow["validAt"] => value ? { precision: value.precision, value: value.value ?? undefined, approximate: value.approximate } : null;

const claimRow = (claim: ChangeClaim, source: ChangeSource | null, group: ChangeRow["group"]): ChangeRow => ({
  id: `${group}:${claim.claimId}`,
  group,
  proposition: claim.proposition,
  validAt: point(claim.validAt),
  invalidAt: point(claim.invalidAt),
  published: source?.publicationTime ?? undefined,
  sourceTitle: source?.title || undefined,
  passage: source ? { recordId: source.recordId } : undefined,
});

/** The asset's latest report against the one before it. Nothing when there is no pair. */
export function ReportChanges({ bridge, loupistId, assetId, onOpenSource }: {
  bridge: LoupistBridge;
  loupistId: string;
  assetId: string;
  onOpenSource: (recordId: string) => void;
}) {
  const { t } = useTranslation("assets");
  const [view, setView] = useState<WorldChangesView | "loading" | "error">("loading");
  const [attempt, setAttempt] = useState(0);
  const [waited, setWaited] = useState(false);
  const [notRepeated, setNotRepeated] = useState(false);
  useEffect(() => {
    let live = true;
    setView("loading");
    setWaited(false);
    const timer = window.setTimeout(() => { if (live) setWaited(true); }, 300);
    void bridge.knowledgeWorldChanges({ loupistId, between: { asset: assetId } })
      .then(next => { if (live) setView(next); })
      .catch(() => { if (live) setView("error"); });
    return () => { live = false; window.clearTimeout(timer); };
  }, [bridge, loupistId, assetId, attempt]);

  if (view === "error") return <section className="report-changes" aria-label={t("reportChanges.title")}>
    <h2>{t("reportChanges.title")}</h2>
    <p role="alert">{t("reportChanges.error")}<Button type="button" variant="outline" size="sm" onClick={() => setAttempt(value => value + 1)}>{t("reportChanges.retry")}</Button></p>
  </section>;
  if (view === "loading") return waited ? <section className="report-changes" aria-label={t("reportChanges.title")}><p role="status">{t("reportChanges.loading")}</p></section> : null;
  if (!view.pair) return null;

  const open = (row: ChangeRow) => { if (row.passage) onOpenSource(row.passage.recordId); };
  const showAll = () => t("reportChanges.showAll");
  const from = view.from;
  const to = view.to;
  const named = new Map([...view.endedByTo, ...view.newInTo, ...view.restated, ...view.notRestated].map(claim => [claim.claimId, claim.proposition]));
  const disagreements = view.conflicts.flatMap(conflict => {
    const proposition = named.get(conflict.claimId) ?? named.get(conflict.withClaimId);
    return proposition ? [claimRow({ claimId: conflict.claimId, proposition, relation: "", status: "disputed", validAt: null, invalidAt: null }, to, "disagreement")] : [];
  });
  return <section className="report-changes" aria-label={t("reportChanges.title")}>
    <h2>{t("reportChanges.title")}</h2>
    <p>{[from?.title, from?.publicationTime ? dateValue(from.publicationTime) : undefined].filter(Boolean).join(" · ")}
      {from && to ? " · " : ""}
      {[to?.title, to?.publicationTime ? dateValue(to.publicationTime) : undefined].filter(Boolean).join(" · ")}</p>
    {to?.pending ? <p className="report-changes-pending" role="status">{t("reportChanges.pending")}</p> : null}
    <ChangeGroup title={t("reportChanges.ended")} rows={view.endedByTo.map(claim => claimRow(claim, to, "ended"))} onOpen={open} showAllLabel={showAll} />
    <ChangeGroup title={t("reportChanges.added")} rows={view.newInTo.map(claim => claimRow(claim, to, "added"))} onOpen={open} showAllLabel={showAll} />
    <ChangeGroup title={t("reportChanges.disagree")} rows={disagreements} onOpen={open} showAllLabel={showAll} />
    {view.notRestated.length ? <section aria-label={t("reportChanges.notRepeated")}>
      <h3><button type="button" aria-expanded={notRepeated} onClick={() => setNotRepeated(value => !value)}>{t("reportChanges.notRepeated")} <span>{formatNumber(view.notRestated.length)}</span></button></h3>
      {notRepeated ? <ChangeGroup heading={false} title={t("reportChanges.notRepeated")} rows={view.notRestated.map(claim => claimRow(claim, from, "added"))} onOpen={open} showAllLabel={showAll} /> : null}
    </section> : null}
    <p>{t("reportChanges.both", { count: view.restated.length, formattedCount: formatNumber(view.restated.length) })}</p>
  </section>;
}
