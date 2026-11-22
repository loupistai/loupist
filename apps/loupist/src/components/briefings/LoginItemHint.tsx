import { useState } from "react";
import { XIcon } from "lucide-react";
import { useTranslation } from "../../i18n";
import { useResidentSettings } from "../../app/useResidentSettings";
import type { LoupistBridge } from "../../types";
import { Button } from "../ui/button";

export function LoginItemHint({ bridge, active }: { bridge: LoupistBridge; active: boolean }) {
  const { t } = useTranslation(["briefings", "settings", "app"]);
  const { resident, busy, error, change } = useResidentSettings(bridge);
  const [enabledHere, setEnabledHere] = useState(false);
  const [denied, setDenied] = useState(false);
  const enable = async () => {
    const result = await change(() => bridge.loginItemSet({ enabled: true }));
    setEnabledHere(result?.loginItem === "on");
    setDenied(result?.loginItem === "error" || result?.loginItem === "requiresApproval");
  };
  if (enabledHere && resident?.loginItem === "on") return <p role="status" className="briefings-meta">{t("schedule.willOpenAtLogin")}</p>;
  if (denied && active && (resident?.loginItem === "error" || resident?.loginItem === "requiresApproval")) {
    return <p role="status" className="briefings-meta">{t(resident.loginItem === "error" ? "settings:general.loginItemDenied" : "settings:general.loginItemDisabled")}</p>;
  }
  if (!active || resident?.loginItem !== "off" || resident.loginHintDismissed) return null;
  return <div className="briefings-meta">
    {t("schedule.loginHint")} <Button variant="link" disabled={busy} onClick={() => void enable()}>{t("schedule.openAtLogin")}</Button>
    <Button variant="ghost" size="sm" aria-label={t("app:app.dismiss")} disabled={busy} onClick={() => void change(() => bridge.loginItemHintDismiss())}><XIcon aria-hidden="true" /></Button>
    {error === "save" && <span role="alert">{t("settings:general.saveFailed")}</span>}
  </div>;
}
