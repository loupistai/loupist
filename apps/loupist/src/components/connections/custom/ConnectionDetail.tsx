import { ConnectionAuth } from "./ConnectionAuth";
import { useState } from "react";
import { useTranslation } from "../../../i18n";
import type { CustomConnection, CustomConnectionsBridge } from "../../../contracts/customConnections";
import { Button } from "../../ui/button";
import { Switch } from "../../ui/switch";
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "../../ui/sheet";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "../../ui/dialog";
import { ToolReview } from "./ToolReview";
import "../../../styles/connections.css";

export function ConnectionDetail({ bridge, connection, loupistId, onChanged, onClose, onRemoved }: {
  bridge: CustomConnectionsBridge; connection: CustomConnection; loupistId: string; onChanged: (record: CustomConnection) => void; onClose: () => void; onRemoved: () => void;
}) {
  const { t } = useTranslation("settings");
  const [remove, setRemove] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  async function update(action: () => Promise<unknown>) {
    setBusy(true); setError(false);
    try { await action(); } catch { setError(true); } finally { setBusy(false); }
  }
  return <><Sheet open onOpenChange={open => { if (!open) onClose(); }}><SheetContent className="custom-detail" aria-describedby={undefined}>
    <SheetHeader><SheetTitle>{connection.name}</SheetTitle></SheetHeader>
    <div className="custom-detail-body custom-stack">
      {error ? <p role="alert" className="custom-status negative">{t("custom.error")}</p> : null}
      <label className="custom-switch">{t("custom.enabled")}<Switch aria-label={t("custom.enabled")} checked={connection.enabled} disabled={busy || !loupistId} onCheckedChange={enabled => void update(async () => onChanged(await bridge.customConnectionSetEnabled({ connection_id: connection.id, loupist_id: loupistId, enabled })))} /></label>
      <section><h3>{t("custom.tools")}</h3><ToolReview bridge={bridge} connection={connection} loupistId={loupistId} busy={busy} onChanged={onChanged} onError={() => setError(true)} /></section>
      <section className="custom-stack"><h3>{t("custom.memory")}</h3><label className="custom-switch">{t("custom.saveToMemory")}<Switch aria-label={t("custom.saveToMemory")} checked={!connection.dont_save_to_memory} disabled={busy} onCheckedChange={save => void update(async () => { await bridge.customConnectionSetMemory({ connection_id: connection.id, dont_save_to_memory: !save }); const records = await bridge.customConnectionList({ loupist_id: loupistId }); const next = records.find(record => record.id === connection.id); if (next) onChanged(next); })} /></label><p className="custom-meta">{t("custom.memoryCaption")}</p></section>
      <section className="custom-stack"><h3>{t("custom.connection")}</h3><pre className="custom-command">{connection.transport.kind === "http" ? connection.transport.url : connection.command_line}</pre>
        {connection.transport.kind === "http" ? <ConnectionAuth bridge={bridge} connection={connection} reauthenticate onChanged={onChanged} /> : null}
        {connection.pending ? <>
          <p className="custom-meta">{t("custom.nothingRuns")}</p>
          <p className="custom-meta">{t("custom.envNames")}: {connection.env_keys.join(", ") || t("custom.none")}</p>
          <p className="custom-meta">{t("custom.headerNames")}: {connection.header_names.join(", ") || t("custom.none")}</p>
          <Button disabled={busy} onClick={() => void update(async () => onChanged(await bridge.customConnectionConfirm({ connection_id: connection.id })))}>{t("custom.add")}</Button></> : null}
        <Button variant="destructive" disabled={busy} onClick={() => setRemove(true)}>{t("custom.remove")}</Button>
      </section>
    </div>
  </SheetContent></Sheet>
  <Dialog open={remove} onOpenChange={setRemove}><DialogContent size="sm"><DialogHeader><DialogTitle>{t("custom.removeTitle", { name: connection.name })}</DialogTitle><DialogDescription>{t("custom.removeDescription")}</DialogDescription></DialogHeader><DialogFooter><Button variant="outline" onClick={() => setRemove(false)}>{t("custom.cancel")}</Button><Button variant="destructive" disabled={busy} onClick={() => void update(async () => { await bridge.customConnectionRemove({ connection_id: connection.id }); setRemove(false); onRemoved(); })}>{t("custom.remove")}</Button></DialogFooter></DialogContent></Dialog>
  </>;
}
