import { comparisonTitle, warningReason, WarningAction, WarningDetails } from "./FigureWarnings";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import "@/styles/answer-checks.css";
import i18n, { useTranslation } from "@/i18n";
import { TriangleAlertIcon } from "lucide-react";
import { Popover } from "radix-ui";
import { useCallback, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import type { ChatEvidenceRef, NumberCheck } from "../../types";
import { providerName } from "@/lib/marketProviderName";
import { formatAsOf } from "../../chat/asOf";
import { byteMatchParts, evidenceProvenance, evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { quoteEvidence } from "../../chat/quoteEvidence";
import { QuoteEvidenceBody } from "./QuoteEvidence";
import { calculationCheck, citationNumber, copiedFormula, formulaLines, needsChecking, spokenNumber, unbroken } from "../../chat/numberChecks";
import { calcView, dataRows, exactAmount, matchedConversion, shownExactly } from "../../chat/numberText";
import { narrowWindow } from "../../chat/narrowWindow";
import { wantsExternal } from "../../chat/navigation";
import { Button } from "../ui/button";
import { useOpenProps } from "../navigation/Open";
import { useConversationScroll } from "../ai-elements/conversation";
import { CLOSE_DELAY_MS, OPEN_DELAY_MS, useEvidencePopover } from "./useEvidencePopover";

const t = i18n.getFixedT(null, "chat");

const FORMULA_LINES = 4;

/** The status line under a popover's content (finance-show-the-math.md §2.4). */
function StatusLine({ check, exact, matchDetail, asOf, id }: { check: NumberCheck; exact: string; matchDetail?: string; asOf?: string; id: string }) {
  useTranslation("chat");
  return <>
    {check.status === "differs" && !check.comparisonReplay ? <p id={id} className="number-chip-status" data-status="differs"><TriangleAlertIcon aria-hidden="true" />{t("numberChip.differs", { nearest: check.nearest ?? "", shown: check.text })}</p> : null}
    {check.status === "rounded" && check.exact && !shownExactly(check) ? <p id={id} className="number-chip-status" data-status="rounded">{t("numberChip.rounded", { exact })}</p> : null}
    {matchDetail ? <p id={id} className="number-chip-status" data-status="matched">{matchDetail}</p> : null}
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

/**
 * The marked passage: the check's span into the retained excerpt or, when the
 * excerpt lacks the number, the host's bounded context of the full result.
 * A span wins; offsets that do not decode leave the excerpt unmarked.
 */
function passageParts(evidence: ChatEvidenceRef, span: NumberCheck["span"], context: NumberCheck["context"], lead: number) {
  const parts = span ? byteMatchParts(evidence.excerpt, span) : context ? byteMatchParts(context.text, context) : null;
  // Keep the marked figure visible in the popover’s four-line passage.
  return parts && parts.before.length > lead ? { ...parts, before: `…${parts.before.slice(-lead)}` } : parts;
}

/** The passage with the number marked. Mounted only while the popover is open, so the text is encoded once per open. */
function PassageBody({ evidence, span, context, meta, conversion = false }: { evidence: ChatEvidenceRef; span: NumberCheck["span"]; context: NumberCheck["context"]; meta: ReactNode; conversion?: boolean }) {
  const passage = useMemo(() => passageParts(evidence, span, context, conversion ? 72 : 220), [evidence, span, context, conversion]);
  return <>
    {meta ? <small className="number-chip-meta">{meta}</small> : null}
    <p className="number-chip-passage">{passage ? <>{passage.before}<mark>{passage.match}</mark>{passage.after}</> : evidence.excerpt}</p>
  </>;
}

/** A provider by name, never a runtime id. */
const providerText = (evidence: ChatEvidenceRef) => providerName(evidence.provider);

function InputSource({ source, evidenceRefs, publishedOnly = false }: { source: ChatEvidenceRef; evidenceRefs: readonly ChatEvidenceRef[]; publishedOnly?: boolean }) {
  const link = useOpenProps({ kind: "evidence", evidence: source });
  const number = citationNumber(source, evidenceRefs);
  const chip = number ? <span className="loupist-citation" aria-hidden="true">{number}</span> : null;
  const origin = evidenceProvenance(source, providerText(source) ?? sourceIdentityLabel(source));
  const sourceDate = source.memory_origin ? origin.publication?.label : publishedOnly ? origin.publication?.date : origin.dateLabel;
  const date = sourceDate && unbroken(sourceDate);
  // The provider may wrap above the date; the date and its citation stay together.
  const body = <>
    {origin.attribution ? <span>{origin.attribution}{date ? " · " : ""}</span> : null}
    {date ? <span className="number-chip-date">{date}{chip}</span> : chip}
    {origin.route ? <small className="number-chip-meta number-chip-source-route">{origin.route}</small> : null}
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
            {/* Read right after the amount it qualifies; drawn on its own line below the row. */}
            {input.declaredCurrency ? <dd className="number-chip-currency-note">{t("numberChip.currencyDeclared")}</dd> : null}
            <dd className="number-chip-source">{source
              ? <InputSource source={source} evidenceRefs={evidenceRefs} publishedOnly={input.publishedDateOnly} />
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
  const conversation = useConversationScroll();
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const { state: copyState, copy: copyText } = useCopyFeedback(`${check.evidence_id}:${check.link_index}`);
  const copied = copyState === "copied";
  const timer = useRef<number | null>(null);
  const chip = useRef<HTMLButtonElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const pressed = useRef(false);
  const titleId = useId();
  const statusId = useId();
  const popoverId = useId();
  useEvidencePopover(open, () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    setOpen(false);
    setPinned(false);
  });
  const schedule = (next: boolean, delay: number) => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      if (next) show(false);
      else if (!pinned) setOpen(false);
    }, delay);
  };
  const show = (pin: boolean) => {
    setOpen(true);
    if (!pin) return;
    setPinned(true);
    // A pinned popover is something the reader opened: the thread stays under it as it grows.
    conversation?.holdScrollPosition();
  };
  useEffect(() => () => { if (timer.current !== null) window.clearTimeout(timer.current); }, []);
  useEffect(() => {
    if (!pinned || !open) return;
    // A pinned popover is a dialog: focus moves to its first control.
    const frame = window.requestAnimationFrame(() => (content.current?.querySelector<HTMLElement>(".number-chip-actions button") ?? content.current?.querySelector<HTMLElement>("button"))?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [pinned, open]);
  // In a short window the popover scrolls under its status and actions; a rule then marks where they begin.
  const [overflowing, setOverflowing] = useState(false);
  // The portaled content can mount after `open` changes, so the measurement follows the node itself.
  const [contentNode, setContentNode] = useState<HTMLDivElement | null>(null);
  const contentRef = useCallback((node: HTMLDivElement | null) => { content.current = node; setContentNode(node); }, []);
  useEffect(() => {
    const node = contentNode;
    if (!open || !node) return;
    const measure = () => setOverflowing(node.scrollHeight > node.clientHeight + 1);
    measure();
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : null;
    observer?.observe(node);
    return () => observer?.disconnect();
  }, [open, contentNode]);

  const dismiss = () => {
    setOpen(false);
    setPinned(false);
    chip.current?.focus();
  };
  const controls = () => [...(content.current?.querySelectorAll<HTMLButtonElement>("button:enabled") ?? [])]
    .filter(button => button.tabIndex >= 0);
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
    const focusable = [...scope.querySelectorAll<HTMLElement>(":is(button, a[href], [tabindex]):not(:disabled)")]
      .filter(element => element.tabIndex >= 0);
    const next = focusable[focusable.indexOf(chip.current!) + 1];
    (next ?? chip.current)?.focus();
  };

  const flagged = check.status === "not_found" || Boolean(check.comparisonReplay);
  const unavailable = check.comparisonReplay?.reason === "evidence_unavailable";
  const calculation = calculationCheck(check, evidence);
  const source = flagged ? comparisonTitle(check, evidence) : sourceIdentityLabel(evidence);
  const value = spokenNumber(check.text);
  // A number its source does not hold is never named as coming from it.
  const label = unavailable ? value : flagged ? t("numberChip.labelUnmatched", { value, source })
    : check.kind === "calculated" ? t("numberChip.labelCalculated", { value }) : t("numberChip.labelSourced", { value, source });
  // "Rounded from …" only when the printed value is not the source value itself.
  const conversion = matchedConversion(check);
  const matchDetail = conversion ?? (check.status === "matched" && check.unstated_scale
    ? t(check.unstated_scale === "source" ? "numberChip.unstatedSourceScale" : "numberChip.unstatedAnswerScale")
    : (check.status === "matched" || check.status === "rounded") && check.sourceSign === "outflow"
      ? t("numberChip.sourceOutflow")
      : undefined);
  const status = matchDetail || needsChecking(check) || (check.status === "rounded" && !shownExactly(check)) ? statusId : undefined;
  // Worded only while open: popovers mount lazily (finance-show-the-math.md §3.3).
  const view = useMemo(() => open && check.kind === "calculated" && check.calc ? calcView(check, checks) : undefined, [open, check, checks]);
  const exact = useMemo(() => check.status === "rounded" && check.exact ? exactAmount(check, evidence, checks) : "", [check, evidence, checks]);
  const title = check.kind === "calculated" ? view?.title ?? check.calc?.title ?? evidenceTitle(evidence)
    : check.kind === "data" ? rowTitle(check) ?? evidenceTitle(evidence)
    : evidenceTitle(evidence);
  const rows = useMemo(() => open && check.kind === "data" ? dataRows(check, evidence, checks, title) : [], [open, check, evidence, checks, title]);
  const observation = useMemo(() => open && check.context && (check.kind === "data" || evidence.tool_name === "dataset_query")
    ? byteMatchParts(check.context.text, check.context) : null, [open, check.context, check.kind, evidence.tool_name]);
  const symbolField = check.fields?.find((field) => field.key === "symbol")?.value;
  const quote = useMemo(() => open && !observation && (check.kind === "passage" || check.kind === "data")
    ? quoteEvidence(evidence, { pointer: check.pointer, asset, symbol: symbolField })
    : null, [open, observation, check.kind, check.pointer, evidence, asset, symbolField]);
  const checked = quote && quote.rows.length === 0 ? hostChecked(check) : undefined;
  // The head must not name a different ticker than the row the pointer selected.
  const headTitle = quote?.focus && title !== quote.focus.symbol && title !== evidenceTitle(evidence) ? evidenceTitle(evidence) : title;
  const headValue = check.kind === "calculated" ? (view ? view.value : check.text) : undefined;
  const cell = check.pointer ? check.fields?.find((field) => field.matched) : undefined;
  const stepLink = useOpenProps({ kind: "evidence", evidence });
  const sourceLink = useOpenProps({ kind: "evidence", evidence,
    ...(evidence.kind === "saved_context" && evidence.read_ref ? { figureCheck: check } : {}),
    cell: cell && check.pointer ? { pointer: check.pointer, key: cell.key, value: cell.value } : undefined });
  const closeThen = (action: (event: MouseEvent<HTMLElement>) => void) => (event: MouseEvent<HTMLElement>) => {
    action(event);
    setOpen(false);
    setPinned(false);
  };
  // Calculation and step evidence resolves to its inline step, which only an answer has.
  const showStep = closeThen(stepLink.activate);
  const origin = open ? evidenceProvenance(evidence, check.kind === "data" && !flagged ? providerText(evidence) ?? source : source) : null;
  const passageMeta = origin ? [origin.attribution, origin.publication?.label ?? origin.fetchLabel].filter(Boolean).join(" · ") : "";
  const dataMeta = !evidence.memory_origin && origin?.publication && origin.fetched
    ? t("numberChip.filedFetched", { provider: origin.name, filed: unbroken(origin.publication.date), fetched: unbroken(origin.fetched) })
    : passageMeta;

  return <Popover.Root open={open} onOpenChange={(next) => { if (!next) { setOpen(false); setPinned(false); } }}>
    <Popover.Anchor asChild>
      <button ref={chip} type="button" className="number-chip" data-testid="number-chip" data-status={check.status} data-kind={check.kind}
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
        onAuxClick={unavailable ? undefined : sourceLink.props.onAuxClick}
        onClick={(event) => {
          if (timer.current !== null) window.clearTimeout(timer.current);
          if (!unavailable && sourceLink.external && wantsExternal(event)) { setOpen(false); setPinned(false); sourceLink.activate(event); return; }
          show(true);
        }}>
        {children}
        {/* A quiet mark after a number its source does not hold, so it never reads as checked. */}
        {check.status === "not_found" ? <span className="number-chip-unmatched" aria-hidden="true" /> : null}
      </button>
    </Popover.Anchor>
    {status ? <span id={statusId} hidden>{matchDetail ?? (flagged ? warningReason(check, calculation) : check.status === "differs" ? t("numberChip.differs", { nearest: check.nearest ?? "", shown: check.text }) : t("numberChip.rounded", { exact }))}</span> : null}
    {open ? <Popover.Portal>
      {/* A non-modal dialog whether opened by hover, focus or click: it holds buttons, so it is never a tooltip. */}
      <Popover.Content ref={contentRef} id={popoverId} role="dialog" aria-modal={false} aria-labelledby={titleId}
        className="number-chip-popover" data-kind={check.kind} data-overflow={overflowing || undefined} side="top" align="start" sideOffset={6}
        // Stay under the titlebar and above the composer. A short window scrolls the body; the actions stay pinned.
        collisionPadding={popoverCollisionPadding(chip.current)}
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
          <strong id={titleId}>{flagged ? source : check.kind === "step" ? t("numberChip.fromStep") : headTitle}</strong>
          {!flagged && headValue ? <span className="gg-num">{headValue}</span> : null}
        </div>
        {flagged && evidence.memory_origin && passageMeta ? <small className="number-chip-meta">{passageMeta}</small> : null}
        {flagged ? <WarningDetails check={check} calculation={calculation} /> : <>
        {quote ? <QuoteEvidenceBody read={quote} evidence={evidence} mode="cell" checked={checked} /> : null}
        {!quote && check.kind === "calculated" && check.calc && view ? <CalculatedBody check={check} view={view} evidenceRefs={evidenceRefs} /> : null}
        {!quote && check.kind === "passage" ? <PassageBody evidence={evidence} span={check.span} context={check.context}
          meta={passageMeta} conversion={Boolean(conversion)} /> : null}
        {!quote && (check.kind === "data" || observation) ? <>
          {observation ? <p className="number-chip-observation">{observation.before}<mark>{observation.match}</mark>{observation.after}</p>
            : rows.length ? <table className="number-chip-row"><tbody>
            {rows.map((row) => <tr key={row.key}><th scope="row">{row.label}</th><td className="gg-num">{row.matched ? <mark>{row.value}</mark> : row.value}</td></tr>)}
          </tbody></table> : null}
          {dataMeta ? <small className="number-chip-meta">{dataMeta}</small> : null}
        </> : null}
        {check.kind === "step" && !observation ? <p className="number-chip-meta">{evidenceTitle(evidence)}</p> : null}
        </>}
        {origin?.route && (!quote || flagged) ? <small className="number-chip-meta">{origin.route}</small> : null}
        {/* The status and actions stay in view when a short window makes the popover scroll. */}
        <div className="number-chip-foot">
          <StatusLine check={check} exact={exact} matchDetail={matchDetail} asOf={asOf} id={`${statusId}-line`} />
          <div className="number-chip-actions">
            {flagged ? <WarningAction check={check} evidence={evidence} close={() => { setOpen(false); setPinned(false); }} /> : quote ? null : check.kind === "calculated" || check.kind === "step"
              ? stepLink.available ? <Button type="button" variant="ghost" size="sm" onClick={showStep}>{t("numberChip.showStep")}</Button> : null
              : sourceLink.available ? <Button type="button" variant="ghost" size="sm" onClick={closeThen(sourceLink.activate)} onAuxClick={sourceLink.props.onAuxClick}>{t("numberChip.openEvidence")}</Button> : null}
            {!flagged && check.kind === "calculated" && !check.calc?.grid ? <Button type="button" variant="ghost" size="sm" onClick={() => {
              void copyText(copiedFormula(check, evidenceRefs));
            }}>{copied ? t("numberChip.copied") : t("numberChip.copyFormula")}</Button> : null}
          </div>
        </div>
      </Popover.Content>
    </Popover.Portal> : null}
  </Popover.Root>;
}

/** Pixels to keep clear above the bottom edge so the popover does not cover the composer. */
/** Below 640 px of window height the composer leaves too little room, so the popover may cover it. */
const SHORT_WINDOW_HEIGHT = 640;
export function popoverCollisionPadding(chip: HTMLElement | null) {
  if (window.innerHeight < SHORT_WINDOW_HEIGHT) return { top: 56, right: 8, bottom: 8, left: 8 };
  return { top: 112, right: 8, bottom: composerClearance(chip), left: 8 };
}

function composerClearance(chip: HTMLElement | null): number {
  const shell = chip?.closest(".chat-workspace")?.querySelector(".composer-shell");
  if (!(shell instanceof HTMLElement)) return 8;
  return Math.max(8, Math.round(window.innerHeight - shell.getBoundingClientRect().top + 8));
}

/** A number link while its answer streams: a subtle underline, no popover, no layout shift. */
export function PendingNumber({ children }: { children: ReactNode }) {
  return <span className="number-chip" data-testid="number-chip" data-status="pending">{children}</span>;
}
