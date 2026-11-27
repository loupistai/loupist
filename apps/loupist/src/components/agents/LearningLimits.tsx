import { useEffect, useId, useState } from "react";
import { useTranslation } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { notifyConfigurationChanged, useConfiguration } from "../../app/useConfiguration";
import { commandErrorCode } from "../../lib/errors";
import type { LoupistBridge } from "../../types";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

type Limit = "learning_daily_requests" | "learning_program_cap";

/** The host's refusal of a learning limit, worded from its reason and bounds; null for any other failure. */
export function learningLimitRefusal(cause: unknown, t: (key: string, options?: Record<string, unknown>) => string): string | null {
  if (commandErrorCode(cause) !== "invalid_configuration" || !cause || typeof cause !== "object" || !("detail" in cause)) return null;
  const detail = cause.detail as { reason?: unknown; minimum?: unknown; maximum?: unknown } | null;
  const bound = (value: unknown) => typeof value === "number" ? formatNumber(value) : "";
  if (detail?.reason === "below_learning_update") return t("schedule.learning.limits.allowanceRefused", { minimum: bound(detail.minimum) });
  if (detail?.reason === "learning_program_cap") return t("schedule.learning.limits.capRefused", { minimum: bound(detail.minimum), maximum: bound(detail.maximum) });
  return null;
}

/**
 * Loupist settings › Learning (plan 4.4): the daily learning allowance, inside the
 * background allowance (0 turns learning runs off), and how many programs may be
 * active. The host checks both; its refusal shows under the field it names.
 */
export function LearningLimits({ bridge, loupistId }: { bridge: LoupistBridge; loupistId: string }) {
  const { t } = useTranslation("briefings");
  const config = useConfiguration(bridge, { kind: "project", loupistId });
  const effective = config.snapshot?.effective;
  return <div className="learning-limits">
    <LimitField bridge={bridge} config={config} field="learning_daily_requests" value={effective?.learning_daily_requests}
      label={t("schedule.learning.limits.allowance")} description={t("schedule.learning.limits.allowanceHint")}
      status={effective?.learning_daily_requests === 0 ? t("schedule.learning.limits.off") : undefined} />
    <LimitField bridge={bridge} config={config} field="learning_program_cap" value={effective?.learning_program_cap}
      label={t("schedule.learning.limits.cap")} description={t("schedule.learning.limits.capHint")} />
    {config.error && !config.snapshot ? <p role="alert" className="inline-error">{config.error}</p> : null}
  </div>;
}

function LimitField({ bridge, config, field, value, label, description, status }: {
  bridge: LoupistBridge; config: ReturnType<typeof useConfiguration>; field: Limit; value: number | undefined;
  label: string; description: string; status?: string;
}) {
  const { t } = useTranslation("briefings");
  const id = useId();
  const [draft, setDraft] = useState(value === undefined ? "" : String(value));
  const [refusal, setRefusal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { setDraft(value === undefined ? "" : String(value)); }, [value]);
  const number = Number(draft);
  const valid = draft.trim() !== "" && Number.isSafeInteger(number) && number >= 0;
  const save = async () => {
    const snapshot = config.snapshot;
    if (!snapshot || !valid) return;
    setBusy(true); setRefusal(null);
    try {
      await bridge.configUpdate({ scope: snapshot.scope, expectedRevision: snapshot.revision, set: { [field]: number }, unset: [] });
      notifyConfigurationChanged();
    } catch (cause) {
      setRefusal(learningLimitRefusal(cause, t) ?? t("schedule.learning.limits.saveFailed"));
    } finally { setBusy(false); }
  };
  return <div className="agent-config-field" data-learning-limit={field}>
    <div className="agent-config-label"><label htmlFor={id}>{label}</label><p className="agent-config-description" id={`${id}-hint`}>{description}</p></div>
    <div className="agent-config-control"><div className="agent-config-number">
      <Input id={id} type="number" min={0} step={1} inputMode="numeric" disabled={!config.snapshot || busy} value={draft} aria-invalid={!valid || Boolean(refusal)}
        aria-describedby={[`${id}-hint`, refusal ? `${id}-refusal` : status ? `${id}-status` : ""].filter(Boolean).join(" ")}
        onChange={event => { setDraft(event.target.value); setRefusal(null); }} />
      <Button size="sm" variant="outline" disabled={!config.snapshot || busy || !valid || number === value} onClick={() => void save()}
        aria-label={t("schedule.learning.limits.save", { label })}>{t("schedule.learning.limits.saveShort")}</Button>
    </div></div>
    {refusal ? <p id={`${id}-refusal`} role="alert" className="learning-limit-refusal">{refusal}</p>
      : status ? <p id={`${id}-status`} className="learning-meta">{status}</p> : null}
  </div>;
}
