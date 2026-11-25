import { useState } from "react";
import { isMacPlatform } from "../../lib/platform";
import { Button } from "../ui/button";
import { useTranslation } from "@/i18n";
import type { LoupistBridge } from "../../types";
import type { NotificationSettings } from "../../contracts/generated";
import { useResidentSettings } from "../../app/useResidentSettings";
import { Switch } from "../ui/switch";

const defaultHours = { start: "22:00", end: "07:00" };
const defaults: NotificationSettings = { quietHours: defaultHours, dailyCap: 6, showDetails: true,
  defaults: { crossedLine: "now", filings: "now", briefing: "now", other: "digest", failures: "digest" },
};
const kinds = ["crossedLine", "filings", "briefing", "other", "failures"] as const;

export function NotificationsSettings({ bridge }: { bridge: LoupistBridge }) {
  const { t } = useTranslation("settings");
  const { resident, busy, error, change } = useResidentSettings(bridge);
  const [permissionError, setPermissionError] = useState(false);
  const settings = resident?.notifications ?? defaults;
  const save = (patch: Partial<NotificationSettings>) =>
    void change(() => bridge.briefingResidentSettingsSet({ notifications: { ...settings, ...patch } }));
  const disabled = busy || !resident;

  return <div className="settings-general">
    <div className="settings-row">
      <div><strong>{t("notifications.quietHours")}</strong><p className="settings-row-description">{t("notifications.quietDescription")}</p></div>
      <Switch aria-label={t("notifications.quietHours")} checked={Boolean(settings.quietHours)} disabled={disabled}
        onCheckedChange={enabled => save({ quietHours: enabled ? defaultHours : null })} />
    </div>
    {settings.quietHours && <div className="settings-row">
      <label>{t("notifications.start")} <input type="time" disabled={disabled} value={settings.quietHours.start}
        onChange={event => { if (event.currentTarget.value) save({ quietHours: { ...settings.quietHours!, start: event.currentTarget.value } }); }} /></label>
      <label>{t("notifications.end")} <input type="time" disabled={disabled} value={settings.quietHours.end}
        onChange={event => { if (event.currentTarget.value) save({ quietHours: { ...settings.quietHours!, end: event.currentTarget.value } }); }} /></label>
    </div>}
    <label className="settings-row">
      <strong>{t("notifications.dailyCap")}</strong>
      <select value={settings.dailyCap} disabled={disabled} onChange={event => save({ dailyCap: Number(event.currentTarget.value) })}>
        {Array.from({ length: 25 }, (_, count) => <option key={count} value={count}>{count}</option>)}
      </select>
    </label>
    <div className="settings-row">
      <div><strong>{t("notifications.details")}</strong><p className="settings-row-description">{t("notifications.detailsDescription")}</p></div>
      <Switch aria-label={t("notifications.details")} checked={settings.showDetails} disabled={disabled} onCheckedChange={showDetails => save({ showDetails })} />
    </div>
    <fieldset className="settings-general-block">
      <legend>{t("notifications.defaultTitle")}</legend>
      {kinds.map(kind => <label className="settings-row" key={kind}>
        <strong>{t(`notifications.kind.${kind}`)}</strong>
        <select disabled={disabled} value={settings.defaults[kind]} onChange={event => {
          const value = event.currentTarget.value as "now" | "digest" | "off";
          save({ defaults: { ...settings.defaults, [kind]: value } });
        }}>
          <option value="now">{t("notifications.now")}</option>
          <option value="digest">{t("notifications.grouped")}</option>
          <option value="off">{t("notifications.botOnly")}</option>
        </select>
      </label>)}
    </fieldset>
    {isMacPlatform() && <div className="settings-row">
      <strong>{t("notifications.osPermissions")}</strong>
      <Button variant="outline" onClick={() => {
        setPermissionError(false);
        void bridge.notificationPermissionSettingsOpen().catch(() => setPermissionError(true));
      }}>{t("notifications.openSettings")}</Button>
    </div>}
    {permissionError && <p className="inline-error" role="alert">{t("notifications.openFailed")}</p>}
    {error && <p className="inline-error" role="alert">{t(error === "load" ? "general.loadFailed" : "general.saveFailed")}</p>}
  </div>;
}
