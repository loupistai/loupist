import { useEffect, useState } from "react";
import i18n, { useTranslation } from "@/i18n";
import type { LoupistBridge } from "@/types";
import { CURRENT_NOTICE_VERSION } from "@/bridge/notice";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import "./notice.css";

function NoticeCheckbox({ checked, onChange, disabled = false }: { checked: boolean; onChange: (checked: boolean) => void; disabled?: boolean }) {
  const { t } = useTranslation("onboarding");
  return <label className="notice-acknowledgment"><input data-notice-acknowledgment type="checkbox" checked={checked} disabled={disabled} onChange={event => onChange(event.target.checked)} />{t("notice.understand")}</label>;
}

export function NoticeCard({ checked, onChange }: { checked: boolean; onChange: (checked: boolean) => void }) {
  const { t } = useTranslation("onboarding");
  return <section className="notice-card" data-onboarding-notice aria-labelledby="onboarding-notice-title">
    <h2 id="onboarding-notice-title">{t("notice.title")}</h2><p>{t("notice.body")}</p>
    <NoticeCheckbox checked={checked} onChange={onChange} />
  </section>;
}

function NoticeDialog({ onAcknowledge, onClose, error, loading = false, onRetry }: {
  onAcknowledge?: () => Promise<void>; onClose?: () => void; error?: string | null; loading?: boolean; onRetry?: () => void;
}) {
  const { t } = useTranslation("onboarding");
  const [checked, setChecked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState(false);
  const required = !onClose;
  const acknowledge = async () => {
    if (!checked || busy || !onAcknowledge) return;
    setBusy(true); setSaveError(false);
    try { await onAcknowledge(); } catch { setSaveError(true); } finally { setBusy(false); }
  };
  return <Dialog open onOpenChange={open => { if (!open && !required) onClose?.(); }}>
    <DialogContent data-notice-dialog showCloseButton={!required}
      onEscapeKeyDown={event => { if (required) event.preventDefault(); }}
      onInteractOutside={event => { if (required) event.preventDefault(); }}>
      <DialogHeader><DialogTitle>{t("notice.title")}</DialogTitle></DialogHeader>
      <DialogBody><DialogDescription className="notice-body">{t("notice.body")}</DialogDescription>
        {required && <NoticeCheckbox checked={checked} onChange={setChecked} disabled={busy || loading || !!onRetry} />}
        {(error || saveError) && <p className="inline-error" role="alert">{error || t("notice.saveError")}</p>}
      </DialogBody>
      <DialogFooter>{onRetry ? <Button onClick={onRetry}>{t("wizard.retry")}</Button> : required ? <Button disabled={!checked || busy || loading} onClick={() => void acknowledge()}>{t("wizard.continue")}</Button> : <Button onClick={onClose}>{i18n.t("common:dialog.close")}</Button>}</DialogFooter>
    </DialogContent>
  </Dialog>;
}

export function LaunchNotice({ bridge, enabled = true }: { bridge: LoupistBridge; enabled?: boolean }) {
  const { t } = useTranslation("onboarding");
  const [version, setVersion] = useState<number | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    setLoadError(false);
    void bridge.noticeSettingsGet().then(settings => { if (active) setVersion(settings.notice_acknowledged_version); }).catch(() => { if (active) setLoadError(true); });
    return () => { active = false; };
  }, [bridge, attempt]);
  if (!enabled || (version !== null && version >= CURRENT_NOTICE_VERSION)) return null;
  return <NoticeDialog loading={version === null} error={loadError ? t("notice.loadError") : null} onRetry={loadError ? () => setAttempt(value => value + 1) : undefined}
    onAcknowledge={async () => { const saved = await bridge.noticeAcknowledge(); setVersion(saved.notice_acknowledged_version); }} />;
}

export function NoticeSettingsRow() {
  const { t } = useTranslation("onboarding");
  const [open, setOpen] = useState(false);
  return <><div className="settings-row" data-notice-settings><div><strong>{t("notice.settingsTitle")}</strong><p className="settings-row-description">{t("notice.settingsDescription")}</p></div><Button variant="outline" onClick={() => setOpen(true)}>{t("notice.readAgain")}</Button></div>
    {open && <NoticeDialog onClose={() => setOpen(false)} />}</>;
}
