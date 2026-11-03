import { useEffect, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatTime } from "@/i18n/format";
import type { BriefingSettings, LoupistBridge } from "@/types";
import { Button } from "../ui/button";
import { NativeSelect } from "../ui/native-select";
import { Switch } from "../ui/switch";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription, focusDialogTitle } from "../ui/dialog";
import { deviceTimeZone, friendlyTimeZone, weekdays } from "./scheduledPresentation";

const QUARTER_HOURS = [0, 15, 30, 45];
/** Native time controls follow the operating system, so an English app on a Korean Mac shows 오전. */
const hourName = (hour: number) => formatTime(new Date(2026, 0, 1, hour, 0)).replace(":00", "");

function Clock({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const { t } = useTranslation("briefings");
  const [hourPart, minutePart] = value.split(":").map(Number);
  const hour = Number.isFinite(hourPart) ? hourPart : 8;
  const minute = Number.isFinite(minutePart) ? minutePart : 0;
  const minutes = QUARTER_HOURS.includes(minute) ? QUARTER_HOURS : [...QUARTER_HOURS, minute].sort((a, b) => a - b);
  const setClock = (nextHour: number, nextMinute: number) => onChange(`${String(nextHour).padStart(2, "0")}:${String(nextMinute).padStart(2, "0")}`);
  return <div className="briefings-clock">
    <NativeSelect aria-label={t("hour")} value={hour} onChange={event => setClock(Number(event.target.value), minute)}>
      {Array.from({ length: 24 }, (_, item) => <option key={item} value={item}>{hourName(item)}</option>)}
    </NativeSelect>
    <NativeSelect aria-label={t("minute")} value={minute} onChange={event => setClock(hour, Number(event.target.value))}>
      {minutes.map(item => <option key={item} value={item}>{String(item).padStart(2, "0")}</option>)}
    </NativeSelect>
  </div>;
}

export function ScheduleEditor({ bridge, loupistId, settings, enableOnSave, onClose, onSaved }: {
  bridge: LoupistBridge; loupistId: string; settings: BriefingSettings; enableOnSave?: boolean; onClose: () => void; onSaved: (settings: BriefingSettings) => void;
}) {
  const { t, i18n } = useTranslation("briefings");
  const schedule = settings.schedule;
  const [days, setDays] = useState(schedule.kind === "weekly" ? schedule.days : schedule.kind === "daily" ? [0,1,2,3,4,5,6] : [1,2,3,4,5]);
  const [time, setTime] = useState(schedule.kind === "weekly" || schedule.kind === "daily" ? `${String(schedule.hour).padStart(2,"0")}:${String(schedule.minute).padStart(2,"0")}` : "08:00");
  const zone = schedule.kind === "weekly" || schedule.kind === "daily" ? schedule.timezone : deviceTimeZone();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  // App-wide. Enabling briefings turns it back on in Rust, so the choice is saved after the schedule.
  const [keepRunning, setKeepRunning] = useState<boolean | null>(null);
  useEffect(() => {
    let active = true;
    void bridge.briefingResidentSettingsGet().then(saved => { if (active) setKeepRunning(saved.keepRunning); }).catch(() => {});
    return () => { active = false; };
  }, [bridge]);
  return <Dialog open onOpenChange={open => { if (!open && !busy) onClose(); }}><DialogContent className="briefings-schedule-dialog" onOpenAutoFocus={focusDialogTitle}>
    <DialogHeader><DialogTitle>{t("edit")}</DialogTitle><DialogDescription>{t("zone", { zone: friendlyTimeZone(zone, i18n.language) })}</DialogDescription></DialogHeader>
    <form onSubmit={async event => { event.preventDefault(); if (busy || !days.length || !time) return; setBusy(true); setError(false);
      const [hour, minute] = time.split(":").map(Number);
      try {
        const saved = await bridge.briefingSettingsSet({ loupistId, enabled: enableOnSave ?? settings.enabled, schedule: { kind: "weekly", days: [...days].sort(), hour, minute, timezone: zone } });
        if (keepRunning !== null) await bridge.briefingResidentSettingsSet({ keepRunning });
        onSaved(saved);
      }
      catch { setError(true); setBusy(false); }
    }}>
      <fieldset disabled={busy}><legend>{t("days")}</legend><div className="briefings-days">{[1,2,3,4,5,6,0].map(day => <button type="button" key={day} aria-pressed={days.includes(day)} onClick={() => setDays(current => current.includes(day) ? current.filter(value => value !== day) : [...current,day])}>{weekdays()[day]}</button>)}</div></fieldset>
      <fieldset className="briefings-time" disabled={busy}><legend>{t("time")}</legend><Clock value={time} onChange={setTime} /></fieldset>
      {keepRunning !== null && <label className="briefings-resident"><Switch checked={keepRunning} disabled={busy} onCheckedChange={setKeepRunning} aria-labelledby="briefings-resident-title" aria-describedby="briefings-resident-description" /><span><strong id="briefings-resident-title">{t("keepRunning")}</strong><span className="briefings-meta" id="briefings-resident-description">{t("keepRunningBody")}</span></span></label>}
      <p className="briefings-meta">{t("cost")}</p>{error && <p role="alert">{t("saveError")}</p>}
      <DialogFooter><Button type="button" variant="ghost" disabled={busy} onClick={onClose}>{t("cancel")}</Button><Button type="submit" disabled={busy || !days.length || !time}>{t("save")}</Button></DialogFooter>
    </form>
  </DialogContent></Dialog>;
}
