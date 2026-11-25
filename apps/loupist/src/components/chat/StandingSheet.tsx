import { useEffect, useId, useRef, useState } from "react";
import { XIcon } from "lucide-react";
import { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import { errorMessage } from "../../lib/errors";
import type { BotMemoryListEntry, UsageSummaryRequest } from "../../contracts/generated";
import type { LoupistBridge } from "../../types";
import { Button } from "../ui/button";
import "@/styles/standing-sheet.css";

/** The header button opens the sheet by dispatching this on `window`. */
export const OPEN_STANDING_EVENT = "loupist:open-standing";

const KINDS = {
  preference: "standing.kinds.preference",
  decision: "standing.kinds.decision",
  open: "standing.kinds.open",
  context: "standing.kinds.context",
} as const;

type Bridge = Pick<LoupistBridge, "botMemoryList" | "botMemoryForget" | "botMemoryUpdate" | "usageSummary">;

function recentUsage(now = Date.now()): UsageSummaryRequest {
  const start = new Date(now);
  start.setDate(start.getDate() - 29);
  start.setHours(0, 0, 0, 0);
  return {
    fromMs: start.getTime(),
    toMs: now + 1,
    timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC",
  };
}

type Props = {
  open: boolean;
  loupistId: string;
  bridge: Bridge;
  onClose: () => void;
};

/**
 * Bot memory the user can see. Forget and Edit commit without a model call.
 * Task lines read the usage ledger for this Loupist.
 */
export function StandingSheet({ open, loupistId, bridge, onClose }: Props) {
  const { t } = useTranslation("chat");
  const titleId = useId();
  const root = useRef<HTMLElement>(null);
  const [entries, setEntries] = useState<BotMemoryListEntry[] | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [usage, setUsage] = useState<"loading" | "error" | { taskId: string; title: string; runs: number; requests: number }[]>("loading");
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return undefined;
    let current = true;
    setEntries(null);
    setLoadError(false);
    setEditing(null);
    setActionError(null);
    void bridge.botMemoryList({ loupistId }).then(
      list => { if (current) setEntries(list); },
      () => { if (current) setLoadError(true); },
    );
    setUsage("loading");
    void bridge.usageSummary(recentUsage()).then(
      summary => {
        if (!current) return;
        setUsage(summary.programs.filter(program => program.loupistId === loupistId).map(program => ({
          taskId: program.taskId,
          title: program.title ?? "",
          runs: program.runs,
          requests: program.totals.requests,
        })));
      },
      () => { if (current) setUsage("error"); },
    );
    return () => { current = false; };
  }, [bridge, loupistId, open]);

  useEffect(() => {
    if (!open) return undefined;
    root.current?.focus({ preventScroll: true });
    const opener = document.activeElement;
    const key = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || event.isComposing) return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("keydown", key);
      const button = document.querySelector<HTMLElement>("[data-header-standing]");
      const back = opener instanceof HTMLElement && opener.isConnected ? opener : button;
      back?.focus({ preventScroll: true });
    };
  }, [onClose, open]);

  if (!open) return null;

  const save = (id: string, text: string) => {
    setBusy(id);
    setActionError(null);
    void bridge.botMemoryUpdate({ loupistId, id, text }).then(
      list => { setEntries(list); setEditing(null); setBusy(null); },
      cause => { setActionError(errorMessage(cause, t("standing.saveError"))); setBusy(null); },
    );
  };
  const forget = (id: string) => {
    setBusy(id);
    setActionError(null);
    void bridge.botMemoryForget({ loupistId, id }).then(
      list => { setEntries(list); setEditing(current => current?.id === id ? null : current); setBusy(null); },
      cause => { setActionError(errorMessage(cause, t("standing.saveError"))); setBusy(null); },
    );
  };

  return <>
    <div className="standing-scrim" onClick={onClose} />
    <aside ref={root} className="standing-sheet" role="dialog" aria-modal="true" aria-labelledby={titleId} tabIndex={-1} data-testid="standing-sheet">
      <header className="standing-sheet-top">
        <h2 id={titleId}>{t("standing.label")}</h2>
        <Button type="button" variant="ghost" size="icon-sm" aria-label={t("standing.close")} onClick={onClose}><XIcon aria-hidden="true" /></Button>
      </header>
      <div className="standing-sheet-body">
        <p className="standing-sheet-intro">{t("standing.intro")}</p>
        {loadError ? <p className="standing-error" role="alert">{t("standing.loadError")}</p> : null}
        {actionError ? <p className="standing-error" role="alert">{actionError}</p> : null}
        {entries === null && !loadError ? <p className="standing-sheet-status" role="status">{t("standing.loading")}</p> : null}
        {entries?.length === 0 ? <p className="standing-sheet-status" role="status">{t("standing.empty")}</p> : null}
        {entries && entries.length > 0 ? <ul className="standing-list">{entries.map(entry => {
          const editingThis = editing?.id === entry.id;
          const waiting = busy === entry.id;
          return <li key={entry.id} className="standing-entry">
            <span className="standing-entry-kind">{t(KINDS[entry.kind])}</span>
            {editingThis ? <textarea aria-label={t("standing.editText")} maxLength={300} value={editing.text}
              onChange={event => setEditing({ id: entry.id, text: event.target.value })} /> : <p>{entry.text}</p>}
            <p className="standing-entry-meta">
              <time dateTime={new Date(entry.sinceMs).toISOString()}>{formatDate(entry.sinceMs)}</time>
              {entry.updatedMs !== entry.sinceMs ? <span> · {t("standing.updated", { date: formatDate(entry.updatedMs) })}</span> : null}
            </p>
            <div className="standing-entry-actions">
              {editingThis ? <>
                <Button type="button" size="sm" disabled={waiting || editing.text.trim() === "" || editing.text.trim() === entry.text}
                  onClick={() => save(entry.id, editing.text)}>{waiting ? t("standing.saving") : t("standing.save")}</Button>
                <Button type="button" size="sm" variant="ghost" disabled={waiting} onClick={() => setEditing(null)}>{t("standing.cancel")}</Button>
              </> : <>
                <Button type="button" size="sm" variant="ghost" disabled={waiting} onClick={() => { setEditing({ id: entry.id, text: entry.text }); setActionError(null); }}>{t("standing.edit")}</Button>
                <Button type="button" size="sm" variant="ghost" disabled={waiting} onClick={() => forget(entry.id)}>{t("standing.forget")}</Button>
              </>}
            </div>
          </li>;
        })}</ul> : null}
        <section className="standing-usage" aria-labelledby={`${titleId}-usage`}>
          <h3 id={`${titleId}-usage`}>{t("standing.usage")}</h3>
          {usage === "loading" ? <p className="standing-sheet-status" role="status">{t("standing.loading")}</p> : null}
          {usage === "error" ? <p className="standing-error" role="alert">{t("standing.usageError")}</p> : null}
          {Array.isArray(usage) && usage.length === 0 ? <p className="standing-sheet-status" role="status">{t("standing.usageEmpty")}</p> : null}
          {Array.isArray(usage) && usage.length > 0 ? <ul>{usage.map(line => <li key={line.taskId} className="standing-usage-line">
            <span>{line.title || t("standing.removedTask")}</span>
            <span>{t("standing.runs", { count: line.runs, formattedCount: formatNumber(line.runs) })} · {t("standing.requests", { count: line.requests, formattedCount: formatNumber(line.requests) })}</span>
          </li>)}</ul> : null}
        </section>
      </div>
    </aside>
  </>;
}
