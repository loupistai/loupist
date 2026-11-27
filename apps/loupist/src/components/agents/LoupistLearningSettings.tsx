import { useEffect, useState } from "react";
import { useTranslation } from "../../i18n";
import type { LoupistBridge } from "../../types";
import { errorMessage } from "../../lib/errors";
import { learningDefaultCadence } from "../briefings/scheduledPresentation";
import { useGo } from "../navigation/Open";
import { Button } from "../ui/button";
import { Switch } from "../ui/switch";
import { LearningLimits } from "./LearningLimits";

export function LoupistLearningSettings({ bridge, loupistId, onChanged }: {
  bridge: LoupistBridge; loupistId: string; onChanged: () => void;
}) {
  const { t } = useTranslation("knowledge");
  const { t: schedule } = useTranslation("briefings");
  const go = useGo();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let live = true;
    setEnabled(null); setError(null);
    void bridge.memoryLearningSettingsGet({ loupistId }).then(settings => {
      if (live) setEnabled(settings.learnMethods);
    }).catch(cause => { if (live) setError(errorMessage(cause, t("memoryPage.actionError"))); });
    return () => { live = false; };
  }, [bridge, loupistId, retry, t]);
  const save = async (learnMethods: boolean) => {
    setBusy(true); setError(null);
    try {
      const settings = await bridge.memoryLearningSettingsSet({ loupistId, learnMethods });
      setEnabled(settings.learnMethods); onChanged();
    } catch (cause) { setError(errorMessage(cause, t("memoryPage.actionError"))); }
    finally { setBusy(false); }
  };
  return <div className="loupist-learning-settings">
    <label className="loupist-learning-switch"><span>{t("memoryPage.about.learnMethods")}<small className="learning-meta">{t("memoryPage.about.learnMethodsHint")}</small></span>
      <Switch checked={enabled ?? false} disabled={busy || enabled === null} onCheckedChange={value => void save(value)} />
    </label>
    {error ? <div role="alert" className="inline-error">{error}<Button variant="ghost" size="sm" disabled={busy} onClick={() => setRetry(value => value + 1)}>{t("memoryPage.retry")}</Button></div> : null}
    <LearningLimits bridge={bridge} loupistId={loupistId} />
    <p className="learning-meta">{schedule("schedule.learning.defaultTime", { cadence: learningDefaultCadence() })}</p>
    {go ? <Button variant="link" className="justify-self-start" onClick={() => go({ kind: "schedule", taskId: "" })}>{schedule("schedule.learning.openPrograms")}</Button> : <p>{schedule("schedule.learning.openPrograms")}</p>}
  </div>;
}
