import { useEffect, useState } from "react";
import { useTranslation } from "../../i18n";
import type { LoupistBridge } from "../../types";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Switch } from "../ui/switch";

export function LoupistLearningSettings({ bridge, loupistId, onChanged }: {
  bridge: LoupistBridge; loupistId: string; onChanged: () => void;
}) {
  const { t } = useTranslation("knowledge");
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
  </div>;
}
