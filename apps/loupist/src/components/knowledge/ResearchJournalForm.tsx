import { useId, useState, type FormEvent } from "react";
import type { LoupistBridge, JournalCheck, JournalRecord } from "../../types";
import { useTranslation } from "../../i18n";
import { AppDateField } from "../ui/app-date-field";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { NativeSelect } from "../ui/native-select";
import { Textarea } from "../ui/textarea";

export function ResearchJournalForm({ bridge, loupistId, entry, checkOpen = false, onSaved, onCancel }: {
  bridge: LoupistBridge; loupistId: string; entry?: JournalRecord; checkOpen?: boolean;
  onSaved: (entry: JournalRecord) => void; onCancel: () => void;
}) {
  const { t } = useTranslation("knowledge");
  const id = useId();
  const [judgment, setJudgment] = useState(entry?.judgment ?? "");
  const [reasoning, setReasoning] = useState(entry?.reasoning ?? "");
  const [hasCheck, setHasCheck] = useState(Boolean(entry?.check) || checkOpen);
  const [claim, setClaim] = useState(entry?.check?.claim ?? "");
  const [metric, setMetric] = useState(entry?.check?.metric ?? "");
  const [direction, setDirection] = useState<JournalCheck["direction"]>(entry?.check?.direction ?? "above");
  const [threshold, setThreshold] = useState(entry?.check ? String(entry.check.threshold) : "");
  const [unit, setUnit] = useState(entry?.check?.unit ?? "");
  const [byDate, setByDate] = useState(entry?.check?.byDate ?? "");
  const [confidence, setConfidence] = useState(entry?.check?.confidence === undefined ? "" : String(entry.check.confidence));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const frozen = Boolean(entry?.outcome);
  // A line on an asset page may stand without a date.
  const undatedLine = entry?.thesis?.role === "line";
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy) return;
    setBusy(true); setError(false);
    const check: JournalCheck | null = hasCheck ? { claim, metric, direction, threshold: Number(threshold), ...(byDate ? { byDate } : {}), ...(unit ? { unit } : {}), ...(confidence ? { confidence: Number(confidence) } : {}) } : null;
    try {
      const input = { loupistId, judgment, reasoning, check, entities: entry?.entities ?? [] };
      const saved = entry ? await bridge.journalUpdate({ ...input, journalId: entry.journalId, expectedUpdatedAtMs: entry.updatedAtMs }) : await bridge.journalCreate(input);
      onSaved(saved);
    } catch { setError(true); }
    finally { setBusy(false); }
  }
  return <form className="journal-form" data-testid="journal-form" aria-label={t(entry ? "journal.editEntry" : "journal.newEntry")} onSubmit={event => void submit(event)} onFocus={event => { if (event.target instanceof HTMLElement) event.target.scrollIntoView({ block: "nearest", inline: "nearest" }); }}>
    <label htmlFor={`${id}-judgment`}>{t("journal.judgment")}</label>
    <Textarea id={`${id}-judgment`} value={judgment} onChange={e => setJudgment(e.target.value)} maxLength={1000} rows={3} required autoFocus />
    <label htmlFor={`${id}-reasoning`}>{t("journal.reasoning")}</label>
    <Textarea id={`${id}-reasoning`} value={reasoning} onChange={e => setReasoning(e.target.value)} maxLength={2000} rows={3} />
    {!hasCheck ? <Button type="button" variant="ghost" className="journal-check-toggle" aria-expanded={false} aria-controls={`${id}-check`} disabled={frozen} onClick={() => setHasCheck(true)}>{t("journal.addCheck")}</Button> : null}
    {hasCheck ? <fieldset id={`${id}-check`} className="journal-check-fields" disabled={frozen}>
      <legend>{t("journal.check")}</legend>
      <div className="journal-check-body">
      <label htmlFor={`${id}-claim`}>{t("journal.claim")}</label><Input id={`${id}-claim`} value={claim} onChange={e => setClaim(e.target.value)} maxLength={2000} required />
      <label htmlFor={`${id}-metric`}>{t("journal.metric")}</label><Input id={`${id}-metric`} value={metric} onChange={e => setMetric(e.target.value)} maxLength={200} required />
      <div className="journal-form-grid">
        <label>{t("journal.direction")}<NativeSelect value={direction} onChange={e => setDirection(e.target.value as JournalCheck["direction"])}><option value="above">{t("journal.above")}</option><option value="below">{t("journal.below")}</option></NativeSelect></label>
        <label>{t("journal.threshold")}<Input type="number" step="any" value={threshold} onChange={e => setThreshold(e.target.value)} required /></label>
        <label>{t("journal.unit")}<Input value={unit} onChange={e => setUnit(e.target.value)} maxLength={80} /></label>
        <div className="journal-date"><span>{t("journal.byDate")}</span><AppDateField aria-label={t("journal.byDate")} value={byDate} onChange={setByDate} required={!undatedLine} /></div>
      </div>
      <label htmlFor={`${id}-confidence`}>{t("journal.confidence")}</label><NativeSelect id={`${id}-confidence`} value={confidence} onChange={e => setConfidence(e.target.value)}><option value="">{t("journal.notSure")}</option>{Array.from({ length: 9 }, (_, index) => (index + 1) * 10).map(value => <option key={value} value={value}>{t("journal.confidenceOption", { value })}</option>)}</NativeSelect>
      <Button type="button" variant="link" className="journal-check-remove" disabled={frozen} onClick={() => setHasCheck(false)}>{t("journal.removeCheck")}</Button>
      </div>
    </fieldset> : null}
    <p className="journal-caption">{t("journal.caption")}</p>
    {error ? <p role="alert">{t("journal.saveError")}</p> : null}
    <div className="journal-actions"><Button type="submit" disabled={busy || !judgment.trim()}>{t(busy ? "journal.saving" : "journal.save")}</Button><Button type="button" variant="ghost" disabled={busy} onClick={onCancel}>{t("journal.cancel")}</Button></div>
  </form>;
}
