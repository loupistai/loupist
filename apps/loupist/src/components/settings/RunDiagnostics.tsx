import { useEffect, useRef, useState } from "react";
import { CopyIcon, FolderOpenIcon } from "lucide-react";
import type { LoupistBridge } from "../../types";
import { useTranslation } from "../../i18n";
import { Button } from "../ui/button";

export function RunDiagnostics({ bridge, loupistId }: { bridge: LoupistBridge; loupistId: string | null }) {
  const { t } = useTranslation("settings");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ key: "copied" | "noError" | "failed" | "copyFailed" | "revealed"; error: boolean } | null>(null);
  const generation = useRef(0);
  useEffect(() => { generation.current++; setMessage(null); setBusy(false); }, [loupistId]);
  const act = async (copy: boolean) => {
    const current = generation.current;
    setBusy(true); setMessage(null);
    let copying = false;
    try {
      if (copy) {
        const line = await bridge.runLogsLastError({ loupist_id: loupistId });
        if (current !== generation.current) return;
        if (line === null) { setMessage({ key: "noError", error: false }); return; }
        copying = true;
        await navigator.clipboard.writeText(line);
        if (current === generation.current) setMessage({ key: "copied", error: false });
      } else {
        await bridge.runLogsReveal({ loupist_id: loupistId });
        if (current === generation.current) setMessage({ key: "revealed", error: false });
      }
    } catch {
      if (current === generation.current) setMessage({ key: copying ? "copyFailed" : "failed", error: true });
    } finally { if (current === generation.current) setBusy(false); }
  };
  return <section className="agent-config-group" aria-labelledby="run-diagnostics-title">
    <h4 id="run-diagnostics-title">{t("runDiagnostics.title")}</h4>
    <p>{t("runDiagnostics.description")}</p>
    <div className="agent-config-field">
      <div className="agent-config-label">{t("runDiagnostics.runLogs")}</div>
      <div className="agent-config-control">
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => void act(false)}><FolderOpenIcon aria-hidden="true" />{t("runDiagnostics.reveal")}</Button>
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => void act(true)}><CopyIcon aria-hidden="true" />{t("runDiagnostics.copy")}</Button>
      </div>
    </div>
    {message ? <p role={message.error ? "alert" : "status"}>{t(`runDiagnostics.${message.key}`)}</p> : null}
  </section>;
}
