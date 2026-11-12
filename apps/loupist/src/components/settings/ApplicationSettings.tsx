import { useEffect, useId, useState } from "react";
import { useTranslation } from "../../i18n";
import type { LoupistBridge } from "../../types";
import type { BriefingResidentSettings } from "../../contracts/scheduled";
import { Switch } from "../ui/switch";
import { NoticeSettingsRow } from "../onboarding/Notice";

const PRIVACY_LINES = ["privacyStays", "privacyProvider", "privacyDataSources", "privacyWeb", "privacyMakers", "privacyKeys"] as const;

/**
 * Settings › General: whether Loupist keeps running in the menu bar (schedules and
 * Watching run only while the app process runs), the AI notice, and what leaves this Mac.
 * Turning the switch records an explicit choice that the host never overrides.
 */
export function ApplicationSettings({ bridge }: { bridge: LoupistBridge }) {
  const { t } = useTranslation("settings");
  const [resident, setResident] = useState<BriefingResidentSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const labelId = useId(), descriptionId = useId();
  useEffect(() => {
    let live = true;
    void bridge.briefingResidentSettingsGet().then(settings => { if (live) setResident(settings); }).catch(() => { if (live) setError(t("general.loadFailed")); });
    return () => { live = false; };
  }, [bridge, t]);
  const change = async (next: boolean) => {
    setBusy(true); setError(null);
    try { setResident(await bridge.briefingResidentSettingsSet({ keepRunning: next })); }
    catch { setError(t("general.saveFailed")); }
    finally { setBusy(false); }
  };
  const automatic = resident?.keepRunning === true && (resident.keepRunningChoice ?? null) === null;
  return <div className="settings-general">
    <div className="settings-row" data-keep-running>
      <div><strong id={labelId}>{t("general.keepRunning")}</strong><p className="settings-row-description" id={descriptionId}>{t("general.keepRunningDescription")}</p>
        {automatic ? <p className="settings-row-description" data-keep-running-automatic>{t("general.keepRunningAutomatic")}</p> : null}</div>
      <Switch aria-labelledby={labelId} aria-describedby={descriptionId} checked={resident?.keepRunning ?? false} disabled={resident === null || busy} onCheckedChange={value => void change(value)} />
    </div>
    {error ? <p className="inline-error" role="alert">{error}</p> : null}
    <section className="settings-about" aria-labelledby="settings-about-title">
      <h2 id="settings-about-title" className="settings-section-title">{t("general.aboutTitle")}</h2>
      <NoticeSettingsRow />
      <div className="settings-row settings-privacy" data-privacy>
        <div><strong>{t("general.privacyTitle")}</strong>
          <ul className="settings-privacy-list">{PRIVACY_LINES.map(key => <li key={key}>{t(`general.${key}`)}</li>)}</ul>
        </div>
      </div>
    </section>
  </div>;
}
