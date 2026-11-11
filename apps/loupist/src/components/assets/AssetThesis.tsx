import i18n, { useTranslation } from "@/i18n";
import { formatDate } from "@/i18n/format";
import { useId, useState, type FormEvent, type ReactNode } from "react";
import { EllipsisIcon, PlusIcon, TriangleAlertIcon } from "lucide-react";
import type { ChatThread, LoupistBridge } from "../../types";
import type { AssetLine, AssetPage, AssetReason } from "../../contracts/assets";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { NativeSelect } from "../ui/native-select";
import { Textarea } from "../ui/textarea";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "../ui/dropdown-menu";
import { VerdictChip } from "./VerdictChip";
import { crossedReason, lineValue, midSentence, shortDay, SourceChips, verdictLabel, type CitationNumbers, type EvidenceOpener } from "./assetText";
import { formatAsOf, localDayOf } from "../../chat/asOf";
import { useLookBack } from "../knowledge/lookBack";

const t = i18n.getFixedT(null, "assets");
const tJournal = i18n.getFixedT(null, "knowledge");
const VERDICT_ORDER = ["holding", "weakening", "challenged", "broken"] as const;
/** The latest-move sentence clamps to three lines past this many characters. */
const LONG_SENTENCE = 220;

type Editing = { kind: "reason"; entry?: AssetReason } | { kind: "line"; entry?: AssetLine } | null;

function ItemMenu({ label, onEdit, onArchive, lookBack }: { label: string; onEdit: () => void; onArchive: () => void; lookBack?: { label: string; onSelect: () => void } }) {
  return <DropdownMenu>
    <DropdownMenuTrigger asChild>
      <Button type="button" variant="ghost" size="icon-sm" className="asset-item-menu" aria-label={label}><EllipsisIcon aria-hidden="true" /></Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end">
      <DropdownMenuItem onSelect={onEdit}>{tJournal("journal.edit")}</DropdownMenuItem>
      <DropdownMenuItem onSelect={onArchive}>{tJournal("journal.archive")}</DropdownMenuItem>
      {lookBack ? <DropdownMenuItem onSelect={lookBack.onSelect}>{lookBack.label}</DropdownMenuItem> : null}
    </DropdownMenuContent>
  </DropdownMenu>;
}

/** The inline form that writes a reason or a line as a Journal entry tagged with the asset. */
function ThesisForm({ bridge, loupistId, page, editing, onDone, onCancel }: {
  bridge: LoupistBridge; loupistId: string; page: AssetPage; editing: NonNullable<Editing>;
  onDone: () => void; onCancel: () => void;
}) {
  const id = useId();
  const line = editing.kind === "line" ? editing.entry : undefined;
  const reason = editing.kind === "reason" ? editing.entry : undefined;
  const [text, setText] = useState(reason?.text ?? "");
  const [claim, setClaim] = useState(line?.claim ?? "");
  const [metric, setMetric] = useState(line?.metric ?? "");
  const [direction, setDirection] = useState<"above" | "below">(line?.direction ?? "above");
  const [threshold, setThreshold] = useState(line ? String(line.threshold) : "");
  const [unit, setUnit] = useState(line?.unit ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (busy) return;
    setBusy(true); setError(false);
    const thesis = { assetId: page.assetId, role: editing.kind } as const;
    const check = editing.kind === "line"
      ? { claim, metric, direction, threshold: Number(threshold), ...(unit.trim() ? { unit: unit.trim() } : {}), ...(line?.byDate ? { byDate: line.byDate } : {}) }
      : null;
    const judgment = editing.kind === "line" ? claim : text;
    try {
      const entry = editing.entry;
      if (entry) await bridge.journalUpdate({ loupistId, journalId: entry.journalId, expectedUpdatedAtMs: entry.updatedAtMs, judgment, check, thesis });
      else await bridge.journalCreate({ loupistId, judgment, check, thesis });
      onDone();
    } catch { setError(true); }
    finally { setBusy(false); }
  }
  const valid = editing.kind === "line" ? claim.trim() && metric.trim() && threshold.trim() && Number.isFinite(Number(threshold)) : text.trim();
  return <form className="asset-thesis-form" aria-label={editing.kind === "line" ? t("lines.add") : t("reasons.add")} onSubmit={event => void submit(event)}>
    {editing.kind === "reason" ? <>
      <label htmlFor={`${id}-reason`}>{t("reasons.label")}</label>
      <Textarea id={`${id}-reason`} value={text} onChange={event => setText(event.target.value)} maxLength={1000} rows={2} required autoFocus />
    </> : <>
      <label htmlFor={`${id}-claim`}>{tJournal("journal.claim")}</label>
      <Input id={`${id}-claim`} value={claim} onChange={event => setClaim(event.target.value)} maxLength={1000} required autoFocus />
      <div className="asset-thesis-form-grid">
        <label>{tJournal("journal.metric")}<Input value={metric} onChange={event => setMetric(event.target.value)} maxLength={200} required /></label>
        <label>{tJournal("journal.direction")}<NativeSelect value={direction} onChange={event => setDirection(event.target.value as "above" | "below")}><option value="above">{tJournal("journal.above")}</option><option value="below">{tJournal("journal.below")}</option></NativeSelect></label>
        <label>{tJournal("journal.threshold")}<Input type="number" step="any" value={threshold} onChange={event => setThreshold(event.target.value)} required /></label>
        <label>{tJournal("journal.unit")}<Input value={unit} onChange={event => setUnit(event.target.value)} maxLength={80} /></label>
      </div>
    </>}
    {error ? <p role="alert">{tJournal("journal.saveError")}</p> : null}
    <div className="asset-thesis-form-actions">
      <Button type="submit" size="sm" disabled={busy || !valid}>{tJournal("journal.save")}</Button>
      <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={onCancel}>{tJournal("journal.cancel")}</Button>
    </div>
  </form>;
}

/**
 * The thesis card: the Loupist's read on a four-step ladder, the user's reasons
 * and the lines that would change their mind. Reasons and lines are the user's
 * Journal entries; the read is the Loupist's and moves only in Rust.
 */
export function AssetThesis({ bridge, loupistId, loupistName, page, name, threads, numbers, onChanged, onUndo, onWorkItOut, onOpenEvidence, onOpenChat }: {
  bridge: LoupistBridge;
  loupistId: string;
  loupistName: string;
  page: AssetPage;
  /** The asset's name in the reader's language. */
  name: string;
  threads: readonly ChatThread[];
  /** The page's source numbers, shared with What changed and At a glance. */
  numbers: CitationNumbers;
  onChanged: () => void;
  onUndo: (moveId: string) => void;
  onWorkItOut: () => void;
  onOpenEvidence: EvidenceOpener;
  onOpenChat: (threadId: string) => void;
}) {
  useTranslation(["assets", "knowledge"]);
  const lookBack = useLookBack();
  const [editing, setEditing] = useState<Editing>(null);
  const [expanded, setExpanded] = useState(false);
  const [archiveFailed, setArchiveFailed] = useState(false);
  const titleId = useId();
  const read = page.read;
  const chatTitle = (chatId?: string) => chatId ? threads.find(thread => thread.id === chatId)?.title : undefined;
  const archive = async (journalId: string) => {
    setArchiveFailed(false);
    try {
      await bridge.journalArchive({ loupistId, journalId, archived: true });
      onChanged();
    } catch { setArchiveFailed(true); }
  };
  const finish = () => { setEditing(null); onChanged(); };
  const form = (kind: "reason" | "line") => editing?.kind === kind && !editing.entry
    ? <ThesisForm bridge={bridge} loupistId={loupistId} page={page} editing={editing} onDone={finish} onCancel={() => setEditing(null)} />
    : null;

  const latest = read?.latestMove;
  let sentence: ReactNode = null;
  if (read) {
    if (read.started || !latest) sentence = <p className="asset-read-sentence">{t("verdict.started", { date: shortDay(read.since) })}</p>;
    else if (latest.actor === "user" && latest.undoesMoveId) sentence = <p className="asset-read-sentence">{t("verdict.undone", { verdict: verdictLabel(read.verdict), date: shortDay(latest.factDate) })}</p>;
    else {
      const reason = latest.line?.crossed ? crossedReason(latest.line, latest.factDate) : midSentence(latest.reason);
      const text = `${t("verdict.since", { verdict: verdictLabel(read.verdict), date: shortDay(read.since) })} — ${reason}`;
      const long = text.length > LONG_SENTENCE;
      sentence = <div className="asset-read-sentence-row">
        <p className="asset-read-sentence" data-clamped={long && !expanded || undefined}>
          {text} <SourceChips evidence={latest.evidence} numbers={numbers} onOpen={onOpenEvidence} />
        </p>
        {long && !expanded ? <Button type="button" variant="link" size="sm" onClick={() => setExpanded(true)}>{t("page.more")}</Button> : null}
        {read.undoMoveId ? <Button type="button" variant="link" size="sm" className="asset-undo" onClick={() => onUndo(read.undoMoveId!)}>{t("verdict.undo")}</Button> : null}
      </div>;
    }
  }

  return <section className="asset-thesis" aria-labelledby={titleId}>
    <div className="asset-thesis-head">
      <h2 id={titleId} className="asset-section-label">{t("thesis.title")}</h2>
      {read ? <span className="asset-read-label"><span>{t("thesis.read", { loupist: loupistName })}</span><VerdictChip verdict={read.verdict} loupist={loupistName} /></span> : null}
    </div>
    {archiveFailed ? <p role="alert">{t("error.archive")}</p> : null}
    {/* "Thesis" and "line" are explained once, here; without a thesis the invitation below says it. */}
    {read || page.reasons.length ? <p className="asset-thesis-hint">{t("thesis.hint")}</p> : null}
    {read ? <>
      <p className="asset-ladder" aria-hidden="true" data-verdict={read.verdict}>
        {VERDICT_ORDER.map(step => <span key={step} data-current={step === read.verdict || undefined}>{verdictLabel(step)}</span>)}
      </p>
      {sentence}
    </> : <div className="asset-no-thesis">
      <p>{t("noThesis.body", { name })}</p>
      {editing?.kind !== "reason" ? <div className="asset-no-thesis-actions">
        <Button type="button" onClick={() => setEditing({ kind: "reason" })}><PlusIcon aria-hidden="true" />{t("reasons.add")}</Button>
        <Button type="button" variant="ghost" onClick={onWorkItOut}>{t("noThesis.inChat")}</Button>
      </div> : null}
      {form("reason")}
    </div>}

    {read || page.reasons.length ? <>
      <h3 className="asset-section-label">{t("reasons.title")}</h3>
      <ol className="asset-reasons">
        {page.reasons.map((reason, index) => editing?.kind === "reason" && editing.entry?.journalId === reason.journalId
          ? <li key={reason.journalId}><ThesisForm bridge={bridge} loupistId={loupistId} page={page} editing={editing} onDone={finish} onCancel={() => setEditing(null)} /></li>
          : <li key={reason.journalId} className="asset-reason">
            <span className="gg-num asset-index" aria-hidden="true">{index + 1}</span>
            <div className="asset-reason-body">
              <p className="asset-reason-text">{reason.text}</p>
              <p className="asset-meta">
                {chatTitle(reason.originChatId)
                  ? <button type="button" className="asset-inline-link" onClick={() => onOpenChat(reason.originChatId!)}>{t("reasons.meta", { date: formatDate(reason.createdAtMs, { year: undefined, month: "short", day: "numeric" }), chat: chatTitle(reason.originChatId) })}</button>
                  : t("reasons.written", { date: formatDate(reason.createdAtMs, { year: undefined, month: "short", day: "numeric" }) })}
                {reason.lastMove ? <> · <span className="asset-reason-move" data-verdict={reason.lastMove.to}>{verdictLabel(reason.lastMove.to)} {shortDay(reason.lastMove.factDate)}</span> <SourceChips evidence={reason.lastMove.evidence} numbers={numbers} onOpen={onOpenEvidence} /></> : null}
              </p>
            </div>
            <ItemMenu label={t("page.options")} onEdit={() => setEditing({ kind: "reason", entry: reason })} onArchive={() => void archive(reason.journalId)}
              lookBack={lookBack ? { label: tJournal("journal.lookBackMenu", { date: formatAsOf(localDayOf(reason.createdAtMs)) }), onSelect: () => lookBack({ loupistId, journalId: reason.journalId, judgment: reason.text, createdAtMs: reason.createdAtMs }) } : undefined} />
          </li>)}
      </ol>
      {form("reason")}
    </> : null}

    {read || page.lines.length ? <>
      <h3 className="asset-section-label">{t("lines.title")}</h3>
      {!page.lines.length && editing?.kind !== "line" ? <p className="asset-lines-empty">{t("lines.empty")}</p> : null}
      <ul className="asset-lines">
        {page.lines.map(line => editing?.kind === "line" && editing.entry?.journalId === line.journalId
          ? <li key={line.journalId}><ThesisForm bridge={bridge} loupistId={loupistId} page={page} editing={editing} onDone={finish} onCancel={() => setEditing(null)} /></li>
          : <li key={line.journalId} className="asset-line" data-status={line.status}>
            <span className="asset-line-glyph" aria-hidden="true">{line.direction === "above" ? "▲" : "▼"}</span>
            <div className="asset-reason-body">
              <p className="asset-reason-text">{line.claim}</p>
              <p className="asset-meta asset-line-status">
                {line.status === "crossed" ? <TriangleAlertIcon aria-hidden="true" className="asset-line-warning" /> : null}
                {line.status === "not_checked" || !line.last ? t("lines.notChecked")
                  : line.status === "crossed" ? t("lines.crossed", { date: shortDay(line.last.factDate), value: lineValue(line.last.observed, line.unit) })
                    : t("lines.notCrossed", { value: lineValue(line.last.observed, line.unit), date: shortDay(line.last.factDate) })}
                {line.last ? <> <SourceChips evidence={line.last.evidence} numbers={numbers} onOpen={onOpenEvidence} /></> : null}
              </p>
            </div>
            <ItemMenu label={t("page.options")} onEdit={() => setEditing({ kind: "line", entry: line })} onArchive={() => void archive(line.journalId)} />
          </li>)}
      </ul>
      {form("line")}
      {!editing ? <div className="asset-thesis-add">
        <Button type="button" variant="ghost" size="sm" onClick={() => setEditing({ kind: "reason" })}><PlusIcon aria-hidden="true" />{t("reasons.add")}</Button>
        <Button type="button" variant="ghost" size="sm" onClick={() => setEditing({ kind: "line" })}><PlusIcon aria-hidden="true" />{t("lines.add")}</Button>
      </div> : null}
    </> : null}
  </section>;
}
