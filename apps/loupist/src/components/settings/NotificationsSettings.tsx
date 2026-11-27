import { useState } from "react";
import { isMacPlatform } from "../../lib/platform";
import { Button } from "../ui/button";
import { useTranslation } from "@/i18n";
import { formatNumber, formatTime } from "@/i18n/format";
import type { LoupistBridge } from "../../types";
import type { NotificationSettings } from "../../contracts/generated";
import { useResidentSettings } from "../../app/useResidentSettings";
import { Input } from "../ui/input";
import { SelectField, SelectItem } from "../ui/select";
import { Switch } from "../ui/switch";

const defaultHours = { start: "22:00", end: "07:00" };
const defaults: NotificationSettings = { quietHours: defaultHours, dailyCap: 6, showDetails: true,
  defaults: { crossedLine: "now", filings: "now", briefing: "now", other: "digest", failures: "digest" },
};
const kinds = ["crossedLine", "filings", "briefing", "other", "failures"] as const;

function QuietHoursTime({ label, value, disabled, onChange }: {
  label: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation("common");
  const [draft, setDraft] = useState<string | null>(null);
  const [hours, minutes] = value.split(":").map(Number);
  const display = formatTime(Date.UTC(2000, 0, 1, hours, minutes), "UTC");

  function commit() {
    if (draft === null) return;
    setDraft(null);
    const match = /^(\D*?)(\d{1,2}):(\d{2})(\D*)$/u.exec(draft.trim());
    if (!match) return;
    const prefix = match[1].trim();
    const suffix = match[4].trim();
    if (prefix && suffix) return;
    const period = (prefix || suffix).toUpperCase();
    const hour = Number(match[2]);
    const minute = Number(match[3]);
    if (minute > 59 || hour > 23 || (period && (hour < 1 || hour > 12))) return;
    const am = period === t("time.am").toUpperCase() || period === "AM";
    const pm = period === t("time.pm").toUpperCase() || period === "PM";
    if (period && !am && !pm) return;
    const hour24 = period ? hour % 12 + (pm ? 12 : 0) : hour;
    const next = `${String(hour24).padStart(2, "0")}:${match[3]}`;
    if (next !== value) onChange(next);
  }

  return <label className="settings-quiet-hour">{label}<Input value={draft ?? display} disabled={disabled}
    onChange={event => setDraft(event.currentTarget.value)} onBlur={commit} onKeyDown={event => {
      if (event.key === "Enter") { event.preventDefault(); commit(); }
      if (event.key === "Escape" && draft !== null) { event.stopPropagation(); setDraft(null); }
    }} /></label>;
}

export function NotificationsSettings({ bridge }: { bridge: LoupistBridge }) {
  const { t } = useTranslation("settings");
  const { resident, busy, error, change } = useResidentSettings(bridge);
  const [permissionError, setPermissionError] = useState(false);
  const settings = resident?.notifications ?? defaults;
  const save = (patch: Partial<NotificationSettings>) =>
    void change(() => bridge.briefingResidentSettingsSet({ notifications: { ...settings, ...patch } }));
  const disabled = busy || !resident;
  const quietHours = settings.quietHours;

  return <div className="settings-general">
    <div className="settings-row">
      <div><strong>{t("notifications.quietHours")}</strong><p className="settings-row-description">{t("notifications.quietDescription")}</p></div>
      <Switch aria-label={t("notifications.quietHours")} checked={Boolean(settings.quietHours)} disabled={disabled}
        onCheckedChange={enabled => save({ quietHours: enabled ? defaultHours : null })} />
    </div>
    {quietHours && <div className="settings-row">
      <QuietHoursTime label={t("notifications.start")} disabled={disabled} value={quietHours.start}
        onChange={start => save({ quietHours: { ...quietHours, start } })} />
      <QuietHoursTime label={t("notifications.end")} disabled={disabled} value={quietHours.end}
        onChange={end => save({ quietHours: { ...quietHours, end } })} />
    </div>}
    <label className="settings-row">
      <strong>{t("notifications.dailyCap")}</strong>
      <SelectField compact value={String(settings.dailyCap)} disabled={disabled} onValueChange={value => save({ dailyCap: Number(value) })}>
        {Array.from({ length: 25 }, (_, count) => <SelectItem key={count} value={String(count)}>{formatNumber(count)}</SelectItem>)}
      </SelectField>
    </label>
    <div className="settings-row">
      <div><strong>{t("notifications.details")}</strong><p className="settings-row-description">{t("notifications.detailsDescription")}</p></div>
      <Switch aria-label={t("notifications.details")} checked={settings.showDetails} disabled={disabled} onCheckedChange={showDetails => save({ showDetails })} />
    </div>
    <fieldset className="settings-notification-defaults" data-testid="notification-defaults">
      <legend>{t("notifications.defaultTitle")}</legend>
      <div className="settings-general-block">
        {kinds.map(kind => <label className="settings-row" key={kind}>
          <strong>{t(`notifications.kind.${kind}`)}</strong>
          <SelectField compact disabled={disabled} value={settings.defaults[kind]} onValueChange={value => {
            save({ defaults: { ...settings.defaults, [kind]: value as "now" | "digest" | "off" } });
          }}>
            <SelectItem value="now">{t("notifications.now")}</SelectItem>
            <SelectItem value="digest">{t("notifications.grouped")}</SelectItem>
            <SelectItem value="off">{t("notifications.botOnly")}</SelectItem>
          </SelectField>
        </label>)}
      </div>
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
