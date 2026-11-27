import { useId, useState } from "react";
import { useTranslation } from "../../i18n";
import type { LoupistBridge } from "../../types";
import { useResidentSettings } from "../../app/useResidentSettings";
import { Switch } from "../ui/switch";
import { Disclosure } from "../ui/disclosure";
import { NoticeSettingsRow } from "../onboarding/Notice";
import { Button } from "../ui/button";
import { formatDate } from "../../i18n/format";
import { useLegacyHoldingsFile, useLegacyHoldingsStatus } from "../../app/useLegacyHoldings";

const PRIVACY_LINES = ["privacyStays", "privacyProvider", "privacyDataSources", "privacyWeb", "privacyMakers", "privacyKeys"] as const;

/**
 * Settings › General: whether Loupist keeps running in the menu bar (scheduled tasks
 * run only while the app process runs), the AI notice, what leaves this Mac,
 * and, while the holdings export exists, its date with Show file and Save a copy….
 * Turning the switch records an explicit choice that the host never overrides.
 */
export function ApplicationSettings({ bridge }: { bridge: LoupistBridge }) {
  const { t } = useTranslation("settings");
  const { resident, busy, error, change } = useResidentSettings(bridge);
  const [paneError, setPaneError] = useState(false);
  const labelId = useId(), descriptionId = useId(), loginId = useId(), loginDescription = useId();
  const status = resident?.loginItem;
  const needsPane = status === "requiresApproval" || status === "error";
  const loginHelp = status === "unavailable" ? "loginItemUnavailable" : status === "requiresApproval" ? "loginItemDisabled" : status === "error" ? "loginItemDenied" : null;
  const automatic = resident?.keepRunning === true && (resident.keepRunningChoice ?? null) === null;
  return <div className="settings-general">
    <section className="settings-general-block" aria-labelledby="settings-application-title">
    <h2 id="settings-application-title" className="settings-caption-label">{t("general.applicationTitle")}</h2>
    <div className="settings-row" data-keep-running>
      <div><strong id={labelId}>{t("general.keepRunning")}</strong><p className="settings-row-description" id={descriptionId}>{t("general.keepRunningDescription")}</p>
        {resident?.loginItemTurnedOff && <p className="settings-row-description" role="status">{t("general.loginItemTurnedOff")}</p>}
        {automatic ? <p className="settings-row-description" data-keep-running-automatic>{t("general.keepRunningAutomatic")}</p> : null}</div>
      <Switch aria-labelledby={labelId} aria-describedby={descriptionId} checked={resident?.keepRunning ?? false} disabled={resident === null || busy} onCheckedChange={value => void change(() => bridge.briefingResidentSettingsSet({ keepRunning: value }))} />
    </div>
    <div className="settings-row">
      <div><strong id={loginId}>{t("general.loginItem")}</strong>
        {loginHelp ? <p className="settings-row-description" id={loginDescription}>{t(`general.${loginHelp}`)}{needsPane && <> <Button variant="link" onClick={() => {
          setPaneError(false);
          void bridge.loginItemOpenSettings().catch(() => setPaneError(true));
        }}>{t("general.openLoginItems")}</Button></>}</p> : null}
      </div>
      <Switch aria-labelledby={loginId} aria-describedby={loginHelp ? loginDescription : undefined} checked={status === "on"} disabled={busy || (status !== "on" && status !== "off")}
        onCheckedChange={enabled => void change(() => bridge.loginItemSet({ enabled }))} />
    </div>
    {error || paneError ? <p className="inline-error" role="alert">{t(error === "load" ? "general.loadFailed" : "general.saveFailed")}</p> : null}
    </section>
    <section className="settings-general-block" aria-labelledby="settings-about-title">
      <h2 id="settings-about-title" className="settings-caption-label">{t("general.aboutTitle")}</h2>
      {/* What a person needs in one sentence; where each kind of data goes is one step away. */}
      <div className="settings-privacy" data-privacy>
        <h3>{t("general.privacyTitle")}</h3>
        <p className="settings-privacy-summary">{t("general.privacySummary")}</p>
        <Disclosure className="settings-privacy-detail" title={t("general.privacyDetail")}>
          <dl className="settings-privacy-list">{PRIVACY_LINES.map(key => {
            // The lead phrase before the colon names where it goes; the rest says what.
            const text = t(`general.${key}`);
            const split = text.indexOf(":");
            return <div key={key}><dt>{split > 0 ? text.slice(0, split) : text}</dt><dd>{split > 0 ? text.slice(split + 1).trim() : ""}</dd></div>;
          })}</dl>
        </Disclosure>
      </div>
      <NoticeSettingsRow />
      <LegacyHoldingsSettingsRow bridge={bridge} />
    </section>
  </div>;
}

/** The holdings export, for as long as it exists: its date only, never a path or a count. */
function LegacyHoldingsSettingsRow({ bridge }: { bridge: LoupistBridge }) {
  const { t } = useTranslation(["settings", "app"]);
  const [status] = useLegacyHoldingsStatus(bridge);
  const file = useLegacyHoldingsFile(bridge);
  if (status?.state !== "exported" || status.exportedAtMs === undefined) return null;
  return <>
    <div className="settings-row" data-legacy-holdings-settings>
      <div><strong>{t("general.legacyHoldingsTitle")}</strong><p className="settings-row-description">{t("general.legacyHoldingsDescription", { date: formatDate(status.exportedAtMs) })}</p></div>
      <span className="settings-row-actions">
        <Button variant="outline" className="legacy-holdings-action" aria-disabled={file.busy || undefined} onClick={file.show}>{t("app:app.legacyHoldings.showFile")}</Button>
        <Button variant="outline" className="legacy-holdings-action" aria-disabled={file.busy || undefined} onClick={file.saveCopy}>{t("app:app.legacyHoldings.saveCopy")}</Button>
      </span>
    </div>
    {file.error ? <p className="inline-error" role="alert">
      {t(file.error === "show" ? "app:app.legacyHoldings.showFailed" : "app:app.legacyHoldings.saveFailed")}
      {file.error === "save" ? <> <Button type="button" variant="link" size="sm" className="legacy-holdings-action" aria-disabled={file.busy || undefined} onClick={file.saveCopy}>{t("app:app.tryAgain")}</Button></> : null}
    </p> : null}
  </>;
}
