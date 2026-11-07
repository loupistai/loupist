import "@/styles/answer-checks.css";
import i18n, { useTranslation } from "@/i18n";
import { TriangleAlertIcon } from "lucide-react";
import { Popover } from "radix-ui";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import type { ChatEvidenceRef, NumberCheck } from "../../types";
import { formatAsOf } from "../../chat/asOf";
import { evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { quoteEvidence } from "../../chat/quoteEvidence";
import { QuoteEvidenceBody } from "./QuoteEvidence";
import { citationNumber, copiedFormula, evidenceDate, formulaLines, needsChecking, spokenNumber, unbroken } from "../../chat/numberChecks";
import { calcView, dataRows, exactAmount, shownExactly } from "../../chat/numberText";
import { narrowWindow } from "../../chat/narrowWindow";
import { wantsExternal } from "../../chat/navigation";
import { Button } from "../ui/button";
import { useOpenProps } from "../navigation/Open";

const t = i18n.getFixedT(null, "chat");

/** Hover dwell before the popover opens, and the grace period for moving onto it (X2). */
const OPEN_DELAY_MS = 150;
const CLOSE_DELAY_MS = 120;
const FORMULA_LINES = 4;

/** One number popover or citation card at a time: opening one closes the others. */
const openChips = new Set<() => void>();

/** A data cell the Evidence reader marks when it opens (`pointer` from the check). */
export type EvidenceCell = { pointer: string; key: string; value: string };


/** The status line under a popover's content (finance-show-the-math.md §2.4). */
function StatusLine({ check, exact, asOf, id }: { check: NumberCheck; exact: string; asOf?: string; id: string }) {
  useTranslation("chat");
  return <>
    {check.status === "not_found" ? <p id={id} className="number-chip-status" data-status="not_found"><TriangleAlertIcon aria-hidden="true" />{t("numberChip.notFound")}</p> : null}
    {check.status === "differs" ? <p id={id} className="number-chip-status" data-status="differs"><TriangleAlertIcon aria-hidden="true" />{t("numberChip.differs", { nearest: check.nearest ?? "", shown: check.text })}</p> : null}
    {check.status === "rounded" && check.exact && !shownExactly(check) ? <p id={id} className="number-chip-status" data-status="rounded">{t("numberChip.rounded", { exact })}</p> : null}
    {asOf ? <p className="number-chip-status" data-status="as_of">{t("numberChip.knownAsOf", { date: unbroken(formatAsOf(asOf)) })}</p> : null}
  </>;
}

/** "{entity} · {period}" when the matched row says so. */
function rowTitle(check: NumberCheck) {
  const field = (keys: string[]) => check.fields?.find((item) => keys.includes(item.key) && !item.matched)?.value;
  const entity = field(["corp_name", "entity", "company", "name", "symbol"]);
  const period = field(["period", "fiscal_year", "bsns_year", "date"]);
  const basis = field(["fs_div", "basis"]);
  return entity && (period || basis) ? [entity, [period, basis].filter(Boolean).join(" ")].join(" · ") : undefined;
}

/** A value Rust already checked. Used only when the quote excerpt itself did not parse. */
function hostChecked(check: NumberCheck): { value: string; symbol?: string } | undefined {
  const symbol = check.fields?.find((field) => field.key === "symbol" && field.value)?.value;
  const matched = check.fields?.find((field) => field.matched && field.key !== "symbol" && field.value)?.value;
  const value = matched ?? (check.exact || undefined);
  return value ? { value, symbol } : undefined;
}

/** The marked passage text, from the check's byte offsets into the retained excerpt. */
function passageParts(evidence: ChatEvidenceRef, span: NumberCheck["span"]) {
  if (!span || span.start < 0 || span.end <= span.start) return null;
  const bytes = new TextEncoder().encode(evidence.excerpt);
  if (span.end > bytes.length) return null;
  try {
    const decoder = new TextDecoder("utf-8", { fatal: true });
    // A short window around the number keeps the four-line clamp on the match.
    const before = decoder.decode(bytes.slice(0, span.start));
    return {
      before: before.length > 220 ? `…${before.slice(-220)}` : before,
      match: decoder.decode(bytes.slice(span.start, span.end)),
      after: decoder.decode(bytes.slice(span.end)),
    };
  } catch { return null; }
}

/** The passage with the number marked. Mounted only while the popover is open, so the excerpt is encoded once per open. */
function PassageBody({ evidence, span, meta }: { evidence: ChatEvidenceRef; span: NumberCheck["span"]; meta: ReactNode }) {
  const passage = useMemo(() => passageParts(evidence, span), [evidence, span]);
  return <>
    <small className="number-chip-meta">{meta}</small>
    <p className="number-chip-passage">{passage ? <>{passage.before}<mark>{passage.match}</mark>{passage.after}</> : evidence.excerpt}</p>
  </>;
}

function InputSource({ source, evidenceRefs }: { source: ChatEvidenceRef; evidenceRefs: readonly ChatEvidenceRef[] }) {
  const link = useOpenProps({ kind: "evidence", evidence: source });
  const number = citationNumber(source, evidenceRefs);
  const chip = number ? <span className="loupist-citation" aria-hidden="true">{number}</span> : null;
  const date = evidenceDate(source);
  // The provider may wrap above the date; the date and its citation stay together.
  const body = <>
    <span>{source.provider ?? sourceIdentityLabel(source)}{date ? " · " : ""}</span>
    {date ? <span className="number-chip-date">{date}{chip}</span> : chip}
  </>;
  return link.available ? <button type="button" {...link.props}>{body}</button> : <span>{body}</span>;
}

function CalculatedBody({ check, view, evidenceRefs }: { check: NumberCheck; view: ReturnType<typeof calcView>; evidenceRefs: readonly ChatEvidenceRef[] }) {
  useTranslation("chat");
  const [showAll, setShowAll] = useState(false);
  const calc = check.calc!;
  const lines = view.formulas.flatMap(formulaLines);
  const shown = showAll ? lines : lines.slice(0, FORMULA_LINES);
  return <>
    {calc.grid
      ? <p className="number-chip-grid">{t("numberChip.grid", { rows: calc.grid[0], cols: calc.grid[1] })}</p>
      : <div className="number-chip-formula">
        {shown.map((line, index) => <p key={index}>{line}</p>)}
        {lines.length > FORMULA_LINES && !showAll ? <Button type="button" variant="ghost" size="sm" onClick={() => setShowAll(true)}>{t("numberChip.showAll")}</Button> : null}
      </div>}
    {view.inputs.length ? <>
      <p className="number-chip-caption">{t("numberChip.inputs")}</p>
      <dl className="number-chip-inputs">
        {view.inputs.map((input) => {
          const source = input.evidenceId ? evidenceRefs.find((item) => item.id === input.evidenceId) : undefined;
          return <div key={input.name}>
            <dt title={input.label}>{input.label}</dt>
            <dd className="gg-num">{input.value}</dd>
            <dd className="number-chip-source">{source
              ? <InputSource source={source} evidenceRefs={evidenceRefs} />
              : <span className="number-chip-unlinked">{t("numberChip.notLinked")}</span>}</dd>
          </div>;
        })}
      </dl>
    </> : null}
    <p className="number-chip-caption number-chip-here">{t("numberChip.calculatedHere")}</p>
  </>;
}

/**
 * A number in an answer that Rust checked against its source (W16.3). The
 * number is the control: hovering for a moment (or focusing it) shows where it
 * came from or how it was calculated; a click pins the popover, and Cmd-click
 * (Ctrl elsewhere) or a middle click opens the source page in the system browser.
 * Rust's check decides the status; the renderer never recomputes it.
 */
export function NumberChip({ check, checks = [check], evidence, evidenceRefs, asOf, asset, children }: {
  check: NumberCheck;
  /** Every check of this answer: inputs and grids read their amounts and range from the others. */
  checks?: readonly NumberCheck[];
  evidence: ChatEvidenceRef;
  evidenceRefs: readonly ChatEvidenceRef[];
  asOf?: string;
  /** Exact ticker of the grid row. A quote cell uses it only when it names one saved symbol. */
  asset?: string;
  children: ReactNode;
}) {
  useTranslation("chat");
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);
  const chip = useRef<HTMLButtonElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const pressed = useRef(false);
  const titleId = useId();
  const statusId = useId();
  const popoverId = useId();
  const close = useRef(() => { setOpen(false); setPinned(false); });
  const schedule = (next: boolean, delay: number) => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      if (next) show(false);
      else if (!pinned) setOpen(false);
    }, delay);
  };
  const show = (pin: boolean) => {
    for (const other of openChips) if (other !== close.current) other();
    setOpen(true);
    if (pin) setPinned(true);
  };
  useEffect(() => {
    const own = close.current;
    if (open) openChips.add(own);
    else openChips.delete(own);
    return () => { openChips.delete(own); };
  }, [open]);
  useEffect(() => () => { if (timer.current !== null) window.clearTimeout(timer.current); }, []);
  useEffect(() => {
    if (!pinned || !open) return;
    // A pinned popover is a dialog: focus moves to its first control.
    const frame = window.requestAnimationFrame(() => (content.current?.querySelector<HTMLElement>(".number-chip-actions button") ?? content.current?.querySelector<HTMLElement>("button"))?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [pinned, open]);
  useEffect(() => {
    if (!copied) return;
    const handle = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(handle);
  }, [copied]);
  // In a short window the popover scrolls under its status and actions; a rule then marks where they begin.
  const [overflowing, setOverflowing] = useState(false);
  useEffect(() => {
    const node = content.current;
    if (!open || !node) return;
    const measure = () => setOverflowing(node.scrollHeight > node.clientHeight + 1);
    measure();
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    observer?.observe(node);
    return () => observer?.disconnect();
  }, [open]);

  const dismiss = () => {
    setOpen(false);
    setPinned(false);
    chip.current?.focus();
  };
  const controls = () => [...(content.current?.querySelectorAll<HTMLElement>("button") ?? [])];
  const onChipKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "Escape" && open) { event.stopPropagation(); dismiss(); return; }
    // The popover is portaled, so Tab from the number moves into it as if it followed the number.
    if (event.key !== "Tab" || event.shiftKey || !open) return;
    const first = controls()[0];
    if (!first) return;
    event.preventDefault();
    if (timer.current !== null) window.clearTimeout(timer.current);
    first.focus();
  };
  const onContentKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") { event.stopPropagation(); event.preventDefault(); dismiss(); return; }
    if (event.key !== "Tab") return;
    const buttons = controls();
    if (event.shiftKey) {
      // Shift+Tab from the first control returns to the number.
      if (document.activeElement !== buttons[0]) return;
      event.preventDefault();
      chip.current?.focus();
      return;
    }
    if (document.activeElement !== buttons.at(-1)) return;
    // Tab past the last control closes the popover and continues in the answer.
    event.preventDefault();
    setOpen(false);
    setPinned(false);
    const scope = chip.current?.closest("article") ?? document.body;
    const focusable = [...scope.querySelectorAll<HTMLElement>("button, a[href], [tabindex]:not([tabindex='-1'])")];
    const next = focusable[focusable.indexOf(chip.current!) + 1];
    (next ?? chip.current)?.focus();
  };

  const source = sourceIdentityLabel(evidence);
  const value = spokenNumber(check.text);
  const label = check.kind === "calculated" ? t("numberChip.labelCalculated", { value }) : t("numberChip.labelSourced", { value, source });
  // "Rounded from …" only when the printed value is not the source value itself.
  const status = needsChecking(check) || (check.status === "rounded" && !shownExactly(check)) ? statusId : undefined;
  // Worded only while open: popovers mount lazily (finance-show-the-math.md §3.3).
  const view = useMemo(() => open && check.kind === "calculated" && check.calc ? calcView(check, checks) : undefined, [open, check, checks]);
  const exact = useMemo(() => check.status === "rounded" && check.exact ? exactAmount(check, evidence, checks) : "", [check, evidence, checks]);
  const title = check.kind === "calculated" ? view?.title ?? check.calc?.title ?? evidenceTitle(evidence)
    : check.kind === "data" ? rowTitle(check) ?? evidenceTitle(evidence)
    : evidenceTitle(evidence);
  const rows = useMemo(() => open && check.kind === "data" ? dataRows(check, evidence, checks, title) : [], [open, check, evidence, checks, title]);
  const symbolField = check.fields?.find((field) => field.key === "symbol")?.value;
  const quote = useMemo(() => open && (check.kind === "passage" || check.kind === "data")
    ? quoteEvidence(evidence, { pointer: check.pointer, asset, symbol: symbolField })
    : null, [open, check.kind, check.pointer, evidence, asset, symbolField]);
  const checked = quote && quote.rows.length === 0 ? hostChecked(check) : undefined;
  // The head must not name a different ticker than the row the pointer selected.
  const headTitle = quote?.focus && title !== quote.focus.symbol && title !== evidenceTitle(evidence) ? evidenceTitle(evidence) : title;
  const headValue = check.kind === "calculated" ? (view ? view.value : check.text) : undefined;
  const cell = check.pointer ? check.fields?.find((field) => field.matched) : undefined;
  const stepLink = useOpenProps({ kind: "evidence", evidence });
  const sourceLink = useOpenProps({ kind: "evidence", evidence, cell: cell && check.pointer ? { pointer: check.pointer, key: cell.key, value: cell.value } : undefined });
  const closeThen = (action: (event: MouseEvent<HTMLElement>) => void) => (event: MouseEvent<HTMLElement>) => {
    action(event);
    setOpen(false);
    setPinned(false);
  };
  // Calculation and step evidence resolves to its inline step, which only an answer has.
  const showStep = closeThen(stepLink.activate);
  const published = evidenceDate(evidence);
  const fetched = evidenceDate(evidence, "retrieved_at");

  return <Popover.Root open={open} onOpenChange={(next) => { if (!next) { setOpen(false); setPinned(false); } }}>
    <Popover.Anchor asChild>
      <button ref={chip} type="button" className="number-chip" data-status={check.status} data-kind={check.kind}
        data-number-check={needsChecking(check) ? "check" : undefined}
        aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? popoverId : undefined} aria-label={label} aria-describedby={status}
        onPointerEnter={() => { if (!narrowWindow()) schedule(true, OPEN_DELAY_MS); }}
        onPointerLeave={() => { if (!pinned) schedule(false, CLOSE_DELAY_MS); }}
        onPointerDown={() => { pressed.current = true; }}
        onFocus={() => { if (!pressed.current) show(false); }}
        onBlur={(event) => {
          pressed.current = false;
          if (!pinned && !content.current?.contains(event.relatedTarget as Node | null)) schedule(false, CLOSE_DELAY_MS);
        }}
        onKeyDown={onChipKeyDown}
        onAuxClick={sourceLink.props.onAuxClick}
        onClick={(event) => {
          if (timer.current !== null) window.clearTimeout(timer.current);
          if (sourceLink.external && wantsExternal(event)) { setOpen(false); setPinned(false); sourceLink.activate(event); return; }
          show(true);
        }}>
        {children}
      </button>
    </Popover.Anchor>
    {status ? <span id={statusId} hidden>{check.status === "not_found" ? t("numberChip.notFound") : check.status === "differs" ? t("numberChip.differs", { nearest: check.nearest ?? "", shown: check.text }) : t("numberChip.rounded", { exact })}</span> : null}
    <Popover.Portal>
      {/* A non-modal dialog whether opened by hover, focus or click: it holds buttons, so it is never a tooltip. */}
      <Popover.Content ref={content} id={popoverId} role="dialog" aria-modal={false} aria-labelledby={titleId}
        className="number-chip-popover" data-kind={check.kind} data-overflow={overflowing || undefined} side="top" align="start" sideOffset={6}
        // Stay under the titlebar and above the composer. A short window scrolls the body; the actions stay pinned.
        collisionPadding={{ top: 112, right: 8, bottom: composerClearance(chip.current), left: 8 }}
        onOpenAutoFocus={(event) => event.preventDefault()} onCloseAutoFocus={(event) => event.preventDefault()}
        // Shift+Tab back to the number keeps the popover open; it is the popover's own trigger.
        onFocusOutside={(event) => { if (event.target === chip.current) event.preventDefault(); }}
        onPointerEnter={() => { if (timer.current !== null) window.clearTimeout(timer.current); }}
        onPointerLeave={() => { if (!pinned) schedule(false, CLOSE_DELAY_MS); }}
        onBlur={(event) => {
          // Focus leaving an unpinned popover for anywhere but the number closes it.
          const next = event.relatedTarget as Node | null;
          if (!pinned && next && !content.current?.contains(next) && next !== chip.current) schedule(false, CLOSE_DELAY_MS);
        }}
        onKeyDown={onContentKeyDown}
        onEscapeKeyDown={(event) => { event.preventDefault(); dismiss(); }}>
        <div className="number-chip-head">
          <strong id={titleId}>{check.kind === "step" ? t("numberChip.fromStep") : headTitle}</strong>
          {headValue ? <span className="gg-num">{headValue}</span> : null}
        </div>
        {quote ? <QuoteEvidenceBody read={quote} evidence={evidence} mode="cell" checked={checked} /> : null}
        {!quote && check.kind === "calculated" && check.calc && view ? <CalculatedBody check={check} view={view} evidenceRefs={evidenceRefs} /> : null}
        {!quote && check.kind === "passage" ? <PassageBody evidence={evidence} span={check.span}
          meta={published ? t("numberChip.published", { source, date: published }) : source} /> : null}
        {!quote && check.kind === "data" ? <>
          {rows.length ? <table className="number-chip-row"><tbody>
            {rows.map((row) => <tr key={row.key}><th scope="row">{row.label}</th><td className="gg-num">{row.matched ? <mark>{row.value}</mark> : row.value}</td></tr>)}
          </tbody></table> : null}
          <small className="number-chip-meta">{evidence.published_at && fetched
            ? t("numberChip.filedFetched", { provider: evidence.provider ?? source, filed: published, fetched })
            : [evidence.provider ?? source, fetched].filter(Boolean).join(" · ")}</small>
        </> : null}
        {check.kind === "step" ? <p className="number-chip-meta">{evidenceTitle(evidence)}</p> : null}
        {/* The status and actions stay in view when a short window makes the popover scroll. */}
        <div className="number-chip-foot">
          <StatusLine check={check} exact={exact} asOf={asOf} id={`${statusId}-line`} />
          <div className="number-chip-actions">
            {quote ? null : check.kind === "calculated" || check.kind === "step"
              ? stepLink.available ? <Button type="button" variant="ghost" size="sm" onClick={showStep}>{t("numberChip.showStep")}</Button> : null
              : sourceLink.available ? <Button type="button" variant="ghost" size="sm" onClick={closeThen(sourceLink.activate)} onAuxClick={sourceLink.props.onAuxClick}>{t("numberChip.openEvidence")}</Button> : null}
            {check.kind === "calculated" && !check.calc?.grid ? <Button type="button" variant="ghost" size="sm" onClick={() => {
              void navigator.clipboard?.writeText(copiedFormula(check, evidenceRefs)).then(() => setCopied(true), () => undefined);
            }}>{copied ? t("numberChip.copied") : t("numberChip.copyFormula")}</Button> : null}
          </div>
        </div>
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>;
}

/** Pixels to keep clear above the bottom edge so the popover does not cover the composer. */
function composerClearance(chip: HTMLElement | null): number {
  const shell = chip?.closest(".chat-workspace")?.querySelector(".composer-shell");
  if (!(shell instanceof HTMLElement)) return 8;
  return Math.max(8, Math.round(window.innerHeight - shell.getBoundingClientRect().top + 8));
}

/** The receipt's "numbers to check": scroll to the first such number in the answer and pin its popover. */
export function revealFirstNumberToCheck(messageId: string) {
  const answer = [...document.querySelectorAll<HTMLElement>("[data-message-id]")].find((element) => element.dataset.messageId === messageId);
  const chip = answer?.querySelector<HTMLButtonElement>('[data-number-check="check"]');
  if (!chip) return;
  const reduced = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  chip.scrollIntoView?.({ block: "center", behavior: reduced ? "auto" : "smooth" });
  chip.click();
}

/** A number link while its answer streams: a subtle underline, no popover, no layout shift. */
export function PendingNumber({ children }: { children: ReactNode }) {
  return <span className="number-chip" data-status="pending">{children}</span>;
}
