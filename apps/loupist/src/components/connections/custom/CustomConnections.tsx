import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronRightIcon } from "lucide-react";
import { useTranslation } from "../../../i18n";
import type { CustomConnection } from "../../../contracts/customConnections";
import type { LoupistSummary, LoupistBridge } from "../../../types";
import { Button } from "../../ui/button";
import { NativeSelect } from "../../ui/native-select";
import { AddConnectionDialog } from "./AddConnectionDialog";
import { ConnectionDetail } from "./ConnectionDetail";
import "../../../styles/connections.css";

// Each row names its next step, like the catalog rows' "Set up ›".
const ROW_STATES = {
  signIn: { tone: "warning", status: "custom.signInNeeded", action: "custom.signIn" },
  failed: { tone: "negative", status: "custom.failed", action: "custom.review" },
  review: { tone: "warning", status: "custom.needsReview", action: "custom.review" },
  ready: { tone: "positive", status: "custom.ready", action: "custom.manage" },
} as const;

function rowState(item: CustomConnection): keyof typeof ROW_STATES {
  if (item.auth_state === "sign_in_needed" || item.auth_state === "in_progress") return "signIn";
  if (item.last_error) return "failed";
  return item.needs_review || item.pending ? "review" : "ready";
}

export function CustomConnections({ bridge, loupists, initialLoupistId }: { bridge: LoupistBridge; loupists: LoupistSummary[]; initialLoupistId?: string | null }) {
  const { t } = useTranslation("settings");
  const [loupistId, setLoupistId] = useState(initialLoupistId || loupists[0]?.id || "");
  const [records, setRecords] = useState<CustomConnection[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState(false);
  useEffect(() => {
    if (!loupists.some(loupist => loupist.id === loupistId)) {
      setLoupistId(loupists.find(loupist => loupist.id === initialLoupistId)?.id || loupists[0]?.id || "");
      setSelected(null); setRecords([]);
    }
  }, [loupists, loupistId, initialLoupistId]);
  const revision = useRef(0);
  const refresh = useCallback(async () => {
    const requested = ++revision.current;
    try {
      const next = await bridge.customConnectionList({ loupist_id: loupistId || undefined });
      if (requested === revision.current) { setRecords(next); setError(false); }
    } catch { if (requested === revision.current) setError(true); }
  }, [bridge, loupistId]);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- a request counter, not a DOM node: cleanup invalidates the request that is in flight.
  useEffect(() => { void refresh(); window.addEventListener("focus", refresh); return () => { revision.current++; window.removeEventListener("focus", refresh); }; }, [refresh]);
  const record = records.find(item => item.id === selected);
  return <section className="custom-connections custom-stack" aria-labelledby="custom-connections-title">
    <div className="custom-heading"><h2 id="custom-connections-title">{t("custom.title")}</h2><Button variant="outline" data-custom-add onClick={() => setAdding(true)}>{t("custom.addButton")}</Button></div>
    {loupists.length && records.length ? <label className="custom-field">{t("custom.loupist")}<NativeSelect value={loupistId} onChange={event => { setLoupistId(event.target.value); setSelected(null); setRecords([]); }}>{loupists.map(loupist => <option key={loupist.id} value={loupist.id}>{loupist.name}</option>)}</NativeSelect></label> : null}
    {error ? <p role="alert">{t("custom.error")} <Button variant="link" onClick={() => void refresh()}>{t("custom.tryAgain")}</Button></p> : null}
    {!records.length ? <p className="custom-meta" data-custom-empty>{t("custom.empty")}</p> : <ul className="custom-rows">{records.map(item => { const state = rowState(item); return <li key={item.id}><button type="button" className="custom-row" data-custom-connection={item.id} onClick={() => setSelected(item.id)}><span><strong>{item.name}</strong><small>{t(item.transport.kind === "http" ? "custom.server" : "custom.localCommand")} · {t("custom.allowedCount", { count: item.tools.filter(tool => tool.approval === "always").length })}</small></span><span className="custom-row-end"><span className={`connection-status ${ROW_STATES[state].tone}`}><span aria-hidden="true" />{t(ROW_STATES[state].status)}</span><span className="custom-row-action">{t(ROW_STATES[state].action)}<ChevronRightIcon aria-hidden="true" /></span></span></button></li>; })}</ul>}
    {adding ? <AddConnectionDialog bridge={bridge} loupistId={loupistId} onChanged={() => void refresh()} onClose={() => { setAdding(false); void refresh(); }} /> : null}
    {record ? <ConnectionDetail bridge={bridge} connection={record} loupistId={loupistId} onClose={() => setSelected(null)} onRemoved={() => { setSelected(null); void refresh(); }} onChanged={() => void refresh()} /> : null}
  </section>;
}
