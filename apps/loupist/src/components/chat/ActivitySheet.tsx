import i18n, { useTranslation } from "@/i18n";
import { formatNumber, formatTime } from "@/i18n/format";
import { useCallback, useEffect, useId, useMemo, useRef, useState, type RefObject } from "react";
import { ArrowUpRightIcon, ChevronDownIcon, ChevronRightIcon, FileIcon, ImageIcon, XIcon } from "lucide-react";
import "@/styles/activity-sheet.css";
import { Button } from "../ui/button";
import { useGo, useOpenProps } from "../navigation/Open";
import { useChatLightbox } from "./ChatLightbox";
import { ChatToolResult } from "./ChatToolResult";
import { ChatActivityDetailView } from "./ChatActivityDetailView";
import type { ChatExecutionScope } from "./ChatExecutionContext";
import { LearnedCard } from "./LearnedCard";

import { humanizeToolName } from "../connections/custom/toolTitle";
import { activityMessages, activityTurn, type ActivityGroup, type ActivityStep, type ActivityTurn, type RailData, type RailFile, type RailUsing } from "../../chat/chatRail";
import { answerSourceRow, type AnswerSource } from "../../chat/answerSources";
import { evidenceProvenance, evidenceTitle } from "../../chat/evidence";
import { savedSourceFigure, type ChatDestination } from "../../chat/navigation";
import { receiptDuration } from "../../chat/turnView";
import { useNow } from "../../chat/runStatus";
import { memoryChangeVerb } from "../../chat/memoryActivity";
import { savesMemory } from "../../chat/receipt";
import { useLearnedDigest } from "../../chat/useLearnedDigest";
import { fileSize } from "../../chat/fileSize";
import { knowledgeLocationFromMemoryChange } from "../../knowledge/location";
import { worldReadinessLabels } from "../../knowledge/worldReadiness";
import { modalDialogOpen } from "../../app/shortcuts";
import { chatTitle } from "../../app/chatTitle";
import type { ChatEvidenceRef, ChatMessage, ChatThread, LoupistBridge, LoupistSummary, ChatSaveActivity, MemoryUpdateChange } from "../../types";
import { focusWouldScroll } from "../../lib/revealFocused";

const t = i18n.getFixedT(null, "chat");

/**
 * The seam other areas use to open the sheet: dispatch this on `window`.
 * `messageId` selects the reply, `tab` the section; `toggle` (the page header's
 * button) closes a sheet that is already open.
 */
export const OPEN_ACTIVITY_EVENT = "loupist:open-activity";
type OpenActivityDetail = { messageId?: string; tab?: "steps" | "sources" | "learned" | "connections"; toggle?: boolean; figureIndex?: number };

export type ActivityTab = "activity" | "learned" | "using";
const TABS: ActivityTab[] = ["activity", "learned", "using"];
const tabFor = (tab: OpenActivityDetail["tab"]): ActivityTab | undefined =>
  tab === "learned" ? "learned" : tab === "connections" ? "using" : tab === "steps" || tab === "sources" ? "activity" : undefined;

/** The sheet's width, and the chat it must leave readable beside it (the docked-panel rule). */
const SHEET_WIDTH = 372;
const CHAT_MIN_WIDTH = 420;
const MAX_EARLIER = 6;
const MAX_TARGETS = 3;

type SheetState = { open: boolean; tab: ActivityTab; messageId: string | null; section: "sources" | null; request: number; figureIndex?: number };

/**
 * Open state, placement and the `loupist:open-activity` listener for one chat. The
 * sheet docks beside the conversation when the chat keeps at least 420 px, and
 * slides over it from the right below that. It never opens by itself.
 */
export function useActivitySheet(active: boolean) {
  const layoutRef = useRef<HTMLElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const [state, setState] = useState<SheetState>({ open: false, tab: "activity", messageId: null, section: null, request: 0 });
  const [width, setWidth] = useState(0);
  // An overlaying sheet ends above the composer, so send, stop and the model menu stay in reach.
  const [composerClearance, setComposerClearance] = useState(0);
  useEffect(() => {
    const element = layoutRef.current;
    if (!element) return undefined;
    const observer = new ResizeObserver(() => update());
    let composer: Element | null = null;
    const update = () => {
      setWidth(element.clientWidth);
      const shell = element.querySelector(".composer-shell");
      if (shell !== composer) { if (composer) observer.unobserve(composer); composer = shell; if (shell) observer.observe(shell); }
      setComposerClearance(shell ? Math.max(0, Math.round(element.getBoundingClientRect().bottom - shell.getBoundingClientRect().top)) : 0);
    };
    update();
    observer.observe(element);
    return () => observer.disconnect();
  }, [state.open]);
  useEffect(() => {
    if (!active) return undefined;
    const listener = (event: Event) => {
      const detail: OpenActivityDetail = (event as CustomEvent<OpenActivityDetail | undefined>).detail ?? {};
      const focused = document.activeElement;
      setState((current) => {
        if (detail.toggle && current.open) return { ...current, open: false };
        if (!current.open) opener.current = focused instanceof HTMLElement ? focused : null;
        return {
          open: true,
          tab: tabFor(detail.tab) ?? (current.open ? current.tab : "activity"),
          messageId: detail.messageId ?? null,
          section: detail.tab === "sources" ? "sources" : null,
          request: current.request + 1,
          figureIndex: detail.figureIndex,
        };
      });
    };
    window.addEventListener(OPEN_ACTIVITY_EVENT, listener);
    return () => window.removeEventListener(OPEN_ACTIVITY_EVENT, listener);
  }, [active]);
  const close = useCallback(() => setState((current) => current.open ? { ...current, open: false } : current), []);
  const setTab = useCallback((tab: ActivityTab) => setState((current) => ({ ...current, tab, section: null, figureIndex: undefined })), []);
  const selectMessage = useCallback((messageId: string) => setState((current) => ({ ...current, tab: "activity", messageId, section: null, figureIndex: undefined })), []);
  const docked = width > 0 && width >= SHEET_WIDTH + CHAT_MIN_WIDTH;
  return { layoutRef: layoutRef as RefObject<HTMLElement>, opener, ...state, open: state.open && active, placement: docked ? "docked" as const : "overlay" as const, composerClearance, close, setTab, selectMessage };
}

export type ActivitySheetControl = ReturnType<typeof useActivitySheet>;

type Props = {
  sheet: ActivitySheetControl;
  loupist: LoupistSummary;
  messages: readonly ChatMessage[];
  threads?: readonly ChatThread[];
  running: boolean;
  rail: RailData;
  bridge: LoupistBridge;
  execution: ChatExecutionScope | null;
  onOpenUsing: (item: RailUsing) => void;
  onOpenFile: (path: string) => void;
  /** Opens the chat's materials on the map, with these names selected; absent without a chat map. */
  onShowOnMap?: (ids?: ReadonlySet<string>) => void;
  onWorldChanged?: () => void;
};

/**
 * Everything about how a reply was made, behind the Loupist's face: what it did,
 * read and checked, what it learned or saved, and what it is using. One docked
 * sheet, opened on demand; nothing about process floats over the conversation.
 */
export function ActivitySheet(props: Props) {
  const { sheet } = props;
  useHeaderPressed(sheet.open);
  if (!sheet.open) return null;
  return <OpenSheet {...props} />;
}

/** The page header's activity button reads as pressed while the sheet is open. */
function useHeaderPressed(open: boolean) {
  useEffect(() => {
    const button = document.querySelector<HTMLElement>("[data-header-activity]");
    button?.setAttribute("aria-pressed", String(open));
    if (open) button?.setAttribute("aria-controls", "activity-sheet"); else button?.removeAttribute("aria-controls");
  }, [open]);
  useEffect(() => () => {
    const button = document.querySelector<HTMLElement>("[data-header-activity]");
    button?.setAttribute("aria-pressed", "false");
    button?.removeAttribute("aria-controls");
  }, []);
}

function failedReplyStatus(message: ChatMessage) {
  const failedAt = t("activitySheet.status.failedAt", { time: formatTime(Date.parse(message.created_at)) });
  return message.failure_kind === "rate_limited" ? `${failedAt} · ${t("chatMessageCard.rate_limited")}` : failedAt;
}

function OpenSheet({ sheet, loupist, messages, threads, running, rail, bridge, execution, onOpenUsing, onOpenFile, onShowOnMap, onWorldChanged }: Props) {
  useTranslation("chat");
  const root = useRef<HTMLElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const tabsId = useId();
  const replies = useMemo(() => activityMessages(messages), [messages]);
  const selected = (sheet.messageId ? replies.find((message) => message.id === sheet.messageId) : undefined) ?? replies.at(-1);
  const { close, opener, placement, request, section } = sheet;

  const comparison = selected?.id === sheet.messageId && sheet.figureIndex !== undefined
    ? selected.number_checks?.find(check => check.link_index === sheet.figureIndex) : undefined;
  const result = comparison ? selected?.evidence_refs?.find(evidence => evidence.id === comparison.evidence_id) : undefined;

  // Opening moves focus into the sheet; closing from inside returns it to the control that opened it.
  useEffect(() => {
    if (!result) root.current?.focus({ preventScroll: true });
  }, [request, result]);
  useEffect(() => {
    const element = root.current;
    const from = opener;
    return () => {
      if (!element?.contains(document.activeElement) && document.activeElement !== document.body) return;
      const button = from.current;
      const target = button?.isConnected && !focusWouldScroll(button) ? button : document.querySelector<HTMLElement>("[data-header-activity]");
      target?.focus({ preventScroll: true });
    };
  }, [opener]);
  useEffect(() => {
    if (section !== "sources") return;
    body.current?.querySelector<HTMLElement>("[data-activity-section=sources]")?.scrollIntoView?.({ block: "start" });
  }, [section, request]);
  // Esc closes from inside the sheet, from the page, or anywhere while it overlays; the composer keeps Esc for Stop.
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      // A dialog or a modal panel sheet above it takes Esc first.
      if (event.key !== "Escape" || event.defaultPrevented || event.isComposing || modalDialogOpen() || document.querySelector('[aria-modal="true"]')) return;
      const focused = document.activeElement;
      const fromPage = !focused || focused === document.body || (focused instanceof HTMLElement && focused.matches("[data-header-activity]"));
      if (placement !== "overlay" && !root.current?.contains(focused) && !fromPage) return;
      event.preventDefault();
      close();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [close, placement]);
  // A click outside an overlaying sheet closes it; the header's button toggles it itself.
  useEffect(() => {
    if (placement !== "overlay") return undefined;
    const press = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Element) || root.current?.contains(target) || target.closest("[data-header-activity], [role=dialog], [role=menu]")) return;
      close();
    };
    document.addEventListener("pointerdown", press, true);
    return () => document.removeEventListener("pointerdown", press, true);
  }, [close, placement]);

  const latest = replies.at(-1);
  // The header speaks for the reply; the Map's own processing shows in the Learned tab, never as "still working".
  const status = running ? t(latest?.memorySaving ? "memorySaving.label" : "activitySheet.status.working")
    : latest?.status === "error" ? failedReplyStatus(latest)
    : latest ? t("activitySheet.status.answeredAt", { time: formatTime(Date.parse(latest.created_at)) })
    : t("activitySheet.status.noAnswer");

  // Over a narrow chat the sheet dims what it covers and stops above the composer.
  const clearance = placement === "overlay" ? { bottom: `${sheet.composerClearance}px` } : undefined;
  return <>{placement === "overlay" ? <div className="activity-sheet-scrim" style={clearance} aria-hidden="true" /> : null}<aside ref={root} id="activity-sheet" className="activity-sheet" data-placement={placement} style={clearance} tabIndex={-1} aria-label={t("activitySheet.label")} data-testid="activity-sheet">
    {/* One row: the tabs, "Working" while a reply runs, and close. The page beside it already shows the Loupist's
        face and name, and "What it did" says when the answer came, so the header repeats neither; once the reply
        ends its status is spoken, not shown. */}
    <header className="activity-sheet-top">
      <div className="activity-sheet-tabs" role="tablist" aria-label={t("activitySheet.label")}>
        {TABS.map((tab) => <button key={tab} type="button" role="tab" id={`${tabsId}-${tab}`} aria-selected={sheet.tab === tab} aria-controls={`${tabsId}-panel`}
          data-activity-tab={tab} tabIndex={sheet.tab === tab ? 0 : -1} onClick={() => sheet.setTab(tab)}
          onKeyDown={(event) => {
            // Arrows step through the tabs; Home and End go to the first and last, as in any tab list.
            const at = TABS.indexOf(tab);
            const to = { ArrowRight: (at + 1) % TABS.length, ArrowLeft: (at + TABS.length - 1) % TABS.length, Home: 0, End: TABS.length - 1 }[event.key];
            if (to === undefined) return;
            event.preventDefault();
            const next = TABS[to]!;
            sheet.setTab(next);
            event.currentTarget.parentElement?.querySelector<HTMLElement>(`[data-activity-tab=${next}]`)?.focus();
          }}>{t(`activitySheet.tabs.${tab}`)}</button>)}
      </div>
      <small role="status" className={running ? "activity-sheet-status" : "sr-only"}>{status}</small>
      <Button variant="ghost" size="icon-sm" data-activity-close aria-label={t("activitySheet.close")} title={t("activitySheet.close")} onClick={close}><XIcon aria-hidden="true" /></Button>
    </header>
    <div ref={body} className="activity-sheet-body" role="tabpanel" id={`${tabsId}-panel`} aria-labelledby={`${tabsId}-${sheet.tab}`}>
      {sheet.tab === "activity" && selected && result && comparison &&
        <ChatToolResult evidence={result} comparison={comparison} expanded focusRequest={request}
          onToggle={() => sheet.selectMessage(selected.id)} />}
      {sheet.tab === "activity" ? <ActivityPanel loupistId={loupist.id} key={selected?.id ?? "none"} message={selected} replies={replies} messages={messages} threads={threads} execution={execution} onSelect={sheet.selectMessage} />
        : sheet.tab === "learned" ? <LearnedPanel message={selected} rail={rail} bridge={bridge} loupistId={loupist.id} onShowOnMap={onShowOnMap} onWorldChanged={onWorldChanged} />
        : <UsingPanel rail={rail} onOpenUsing={onOpenUsing} onOpenFile={onOpenFile} onShowOnMap={onShowOnMap} />}
    </div>
  </aside></>;
}

/** "0.4s" under ten seconds, "34s" and "1m 12s" above. */
function durationLabel(milliseconds: number) {
  if (milliseconds < 10_000) return t("ledger.stepSeconds", { value: formatNumber(milliseconds / 1_000, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) });
  return receiptDuration(milliseconds);
}

function ActivityPanel({ loupistId, message, replies, messages, threads, execution, onSelect }: {
  loupistId: string; message: ChatMessage | undefined; replies: readonly ChatMessage[]; messages: readonly ChatMessage[];
  threads?: readonly ChatThread[];
  execution: ChatExecutionScope | null; onSelect: (messageId: string) => void;
}) {
  useTranslation("chat");
  const turn = message ? activityTurn(message) : null;
  if (!message || !turn) return <p className="activity-empty">{t("activitySheet.status.noAnswer")}</p>;
  const earlier = replies.filter((reply) => reply.id !== message.id).slice(-MAX_EARLIER).reverse();
  return <>
    <section className="activity-section" data-activity-section="steps" aria-labelledby={`activity-steps-${turn.messageId}`}>
      <h3 id={`activity-steps-${turn.messageId}`}>
        <span>{t("activitySheet.did.title")}</span>
        <span className="activity-section-meta">{message.status === "error" ? failedReplyStatus(message) : t("activitySheet.turn.this", { time: formatTime(turn.atMs) })}{turn.durationMs !== undefined ? ` · ${durationLabel(turn.durationMs)}` : ""}</span>
      </h3>
      {turn.groups.length || message.progress?.saveActivity ? <ol className="activity-steps" aria-busy={turn.running || undefined}>
        {turn.groups.map((group) => <StepGroup key={group.key} group={group} execution={execution} />)}
        {message.progress?.saveActivity ? <SavingStep activity={message.progress.saveActivity} threads={threads} live={Boolean(message.memorySaving)} skipped={Boolean(message.memory_update?.skipReason)} /> : null}
      </ol> : <p className="activity-empty">{turn.running ? t("activitySheet.did.running") : t("activitySheet.did.empty")}</p>}
    </section>
    <SourcesSection turn={turn} loupistId={loupistId} />
    <ChecksSection turn={turn} loupistId={loupistId} message={message} />
    {earlier.length ? <section className="activity-section" data-activity-section="earlier">
      <h3><span>{t("activitySheet.turn.earlier")}</span></h3>
      <ol className="activity-steps activity-earlier">
        {earlier.map((reply) => <EarlierReply key={reply.id} reply={reply} question={questionBefore(messages, reply)} onSelect={onSelect} />)}
      </ol>
    </section> : null}
  </>;
}

function SavingStep({ activity, threads, live, skipped }: { activity: ChatSaveActivity; threads?: readonly ChatThread[]; live: boolean; skipped: boolean }) {
  const now = useNow(live);
  const end = activity.finishedAtMs ?? (live ? now : null);
  const chat = threads?.find(thread => thread.id === activity.blockingChatId);
  const waited = activity.waitedMs > 0 ? durationLabel(activity.waitedMs) : null;
  return <li className="activity-step" data-state={live ? "running" : skipped ? "bad" : "ok"}>
    <i aria-hidden="true" />
    <div className="activity-step-copy">
      <span className="activity-step-label">{t("memorySaving.label")}</span>
      {waited ? <small className="activity-step-target">{chat
        ? t("memorySaving.waitedForChat", { duration: waited, title: chatTitle(chat) })
        : t("memorySaving.waitedForOthers", { duration: waited })}</small> : null}
    </div>
    {end === null ? <span className="activity-step-time">{t("memorySaving.unfinished")}</span>
      : <time className="activity-step-time">{durationLabel(Math.max(0, end - activity.startedAtMs))}</time>}
  </li>;
}

/** The user's message the reply answered, as its one-line name in "Earlier answers". */
function questionBefore(messages: readonly ChatMessage[], reply: ChatMessage) {
  const index = messages.indexOf(reply);
  for (let at = index - 1; at >= 0; at -= 1) {
    const message = messages[at]!;
    if (message.role === "user" && !message.steer) return message.content.replace(/\s+/gu, " ").trim();
  }
  return "";
}

function EarlierReply({ reply, question, onSelect }: { reply: ChatMessage; question: string; onSelect: (messageId: string) => void }) {
  useTranslation("chat");
  const steps = useMemo(() => activityTurn(reply).stepCount, [reply]);
  const at = reply.progress?.startedAtMs ?? Date.parse(reply.created_at);
  return <li>
    <i aria-hidden="true" />
    <button type="button" className="activity-earlier-row" title={question || undefined} onClick={() => onSelect(reply.id)}>
      <span className="activity-step-label">{question || formatTime(at)}</span>
      <small>{steps ? t("activitySheet.turn.steps", { count: steps, formattedCount: formatNumber(steps) }) : t("activitySheet.turn.noSteps")}</small>
    </button>
    <time>{formatTime(at)}</time>
  </li>;
}

const groupState = (status: ActivityGroup["status"]) => status === "succeeded" ? "ok" : status === "running" ? "running" : "bad";

/** Up to three things the group read, then "+n". */
function targetsLine(steps: readonly ActivityStep[]) {
  const targets = [...new Set(steps.flatMap((step) => step.target ? [step.target] : []))];
  if (targets.length <= MAX_TARGETS) return targets.join(", ");
  return `${targets.slice(0, MAX_TARGETS).join(", ")} ${t("ledger.more", { formattedCount: formatNumber(targets.length - MAX_TARGETS) })}`;
}

function StepGroup({ group, execution }: { group: ActivityGroup; execution: ChatExecutionScope | null }) {
  useTranslation("chat");
  const [open, setOpen] = useState(false);
  const listId = useId();
  const single = group.steps.length === 1 ? group.steps[0]! : null;
  const targets = targetsLine(group.steps);
  return <li className="activity-step" data-state={groupState(group.status)} data-testid="activity-step-group">
    <i aria-hidden="true" />
    <div className="activity-step-copy">
      <span className="activity-step-title">
        <span className="activity-step-label">{group.label}</span>
        {group.steps.length > 1 ? <span className="activity-step-count">{formatNumber(group.steps.length)}</span> : null}
      </span>
      {single ? <StepLine step={single} label={group.label} execution={execution} /> : <>
        {targets ? <small className="activity-step-target" title={targets}>{targets}</small> : null}
        <button type="button" className="activity-step-expand" data-activity-expand aria-expanded={open} aria-controls={open ? listId : undefined} onClick={() => setOpen(!open)}>
          {open ? <ChevronDownIcon aria-hidden="true" /> : <ChevronRightIcon aria-hidden="true" />}
          {open ? t("activitySheet.did.hideEach") : t("activitySheet.did.showEach", { formattedCount: formatNumber(group.steps.length) })}
        </button>
        {open ? <ol id={listId} className="activity-step-members">
          {group.steps.map((step) => <li key={step.id}><StepLine step={step} label={group.label} execution={execution} member /></li>)}
        </ol> : null}
      </>}
    </div>
    <StepTime status={group.status} durationMs={group.durationMs} />
  </li>;
}

function StepTime({ status, durationMs }: { status: ActivityStep["status"]; durationMs?: number }) {
  useTranslation("chat");
  if (status === "running") return <time className="activity-step-time" data-running="">{t("activitySheet.did.running")}</time>;
  return <time className="activity-step-time">{durationMs !== undefined ? durationLabel(durationMs) : ""}</time>;
}

/**
 * One step's line: what it read (a link when the step opened a page), its sources,
 * the reason it failed, and its command output on request. It carries the
 * `data-progress-*` attributes the native work-progress dump reads.
 */
function StepLine({ step, label, execution, member = false }: { step: ActivityStep; label: string; execution: ChatExecutionScope | null; member?: boolean }) {
  useTranslation("chat");
  const [output, setOutput] = useState(false);
  const outputId = useId();
  return <div className="activity-step-line" data-testid="activity-step" data-progress-category={step.category} data-progress-operation={step.operation}
    data-progress-status={step.status} data-progress-action={label} data-member={member || undefined}>
    {step.target ? step.href ? <StepTarget href={step.href} target={step.target} /> : <small className="activity-step-target" data-testid="activity-step-target" title={step.target}>{step.target}</small> : null}
    {member ? <StepTime status={step.status} durationMs={step.durationMs} /> : null}
    {step.reason ? <p className="activity-step-reason">{step.reason}</p> : null}
    {step.evidence.length ? <ul className="activity-step-sources">{step.evidence.map((evidence) => <li key={evidence.id}><EvidenceLink evidence={evidence} /></li>)}</ul> : null}
    {step.detailId && execution ? <>
      <button type="button" className="activity-step-expand" aria-expanded={output} aria-controls={output ? outputId : undefined} onClick={() => setOutput(!output)}>
        {output ? <ChevronDownIcon aria-hidden="true" /> : <ChevronRightIcon aria-hidden="true" />}{t("chatActivityDetailView.outputLabel")}
      </button>
      {output ? <div id={outputId} className="activity-step-output"><ChatActivityDetailView {...execution} detailId={step.detailId} outputRevision={step.outputRevision} /></div> : null}
    </> : null}
  </div>;
}

/** A page a step read opens in the panel browser, or the system browser on Cmd-click. */
function StepTarget({ href, target }: { href: string; target: string }) {
  const link = useOpenProps({ kind: "link", url: href });
  if (!link.available) return <small className="activity-step-target" data-testid="activity-step-target" title={target}>{target}</small>;
  return <button type="button" className="activity-step-target activity-link" data-testid="activity-step-target" title={target} {...link.props}>{target}</button>;
}

function EvidenceLink({ evidence, to }: { evidence: ChatEvidenceRef; to?: ChatDestination }) {
  const link = useOpenProps(to ?? { kind: "evidence", evidence });
  const title = evidenceTitle(evidence);
  const provenance = evidence.memory_origin ? evidenceProvenance(evidence) : null;
  const body = link.available ? <button type="button" className="activity-link" title={title} {...link.props}>{title}</button> : <span title={title}>{title}</span>;
  if (!provenance) return body;
  const meta = [provenance.attribution, provenance.publication?.label].filter(Boolean).join(" · ");
  return <>{body}{meta ? <small className="activity-evidence-meta">{meta}</small> : null}{provenance.route ? <small className="activity-evidence-meta">{provenance.route}</small> : null}</>;
}

function SourcesSection({ turn, loupistId }: { turn: ActivityTurn; loupistId: string }) {
  useTranslation("chat");
  return <section className="activity-section" data-activity-section="sources">
    <h3><span>{t("activitySheet.read.title")}</span>{turn.sources.length ? <span className="activity-section-meta">{formatNumber(turn.sources.length)}</span> : null}</h3>
    {turn.sources.length ? <ol className="activity-sources">
      {turn.sources.map((source, index) => <SourceRow loupistId={loupistId} key={source.evidence.id} source={source} number={index + 1} />)}
    </ol> : <p className="activity-empty">{t("activitySheet.read.empty")}</p>}
  </section>;
}

/** A numbered source; the whole row opens it in Evidence. */
function SourceRow({ source, number, loupistId }: { source: AnswerSource; number: number; loupistId: string }) {
  const { title, to, citations, provenance } = answerSourceRow(source, loupistId);
  const link = useOpenProps(to);
  const meta = [provenance.attribution, provenance.dateLabel, !source.evidence.memory_origin || provenance.attribution ? citations : null].filter(Boolean).join(" · ");
  const content = <>
    <span className="activity-source-number">{formatNumber(number)}</span>
    <span className="activity-source-copy"><b>{title}</b>{meta ? <small>{meta}</small> : null}{provenance.route ? <small>{provenance.route}</small> : null}</span>
    {link.available ? <ArrowUpRightIcon aria-hidden="true" /> : null}
  </>;
  return <li>{link.available ? <button type="button" className="activity-source" {...link.props}>{content}</button> : <div className="activity-source">{content}</div>}</li>;
}

function ChecksSection({ turn, loupistId, message }: { turn: ActivityTurn; loupistId: string; message: ChatMessage }) {
  useTranslation("chat");
  const count = turn.checks.length;
  return <section className="activity-section" data-activity-section="checks">
    <h3><span>{t("activitySheet.checked.title")}</span></h3>
    {count ? <>
      <p className="activity-checks-summary">{t("activitySheet.checked.summary", { count, formattedCount: formatNumber(count) })}</p>
      <ul className="activity-checks">
        {turn.checks.map(({ check, evidence }) => <li key={`${check.link_index}:${check.evidence_id}`} data-status={check.status}>
          <span className="activity-check-figure">{check.text}</span>
          <span className="activity-check-status">{check.status === "unlinked" ? t("numberChip.unlinked") : t(`activitySheet.checked.status.${check.status}`)}</span>
          {evidence ? <span className="activity-check-source"><EvidenceLink evidence={evidence} to={savedSourceFigure(loupistId, message, evidence, check)} /></span> : null}
        </li>)}
      </ul>
    </> : <p className="activity-empty">{t("activitySheet.checked.empty")}</p>}
  </section>;
}

function LearnedPanel({ message, rail, bridge, loupistId, onShowOnMap, onWorldChanged }: {
  message: ChatMessage | undefined; rail: RailData; bridge: LoupistBridge; loupistId: string; onShowOnMap?: (ids?: ReadonlySet<string>) => void;
  onWorldChanged?: () => void;
}) {
  useTranslation("chat");
  const go = useGo();
  const digestId = useId();
  const placeholder = useMemo(() => ({ id: "", role: "assistant", content: "", created_at: "" }) as ChatMessage, []);
  const digest = useLearnedDigest(bridge, loupistId, message ?? placeholder, !!message && savesMemory(message));
  const learned = rail.learned;
  const update = message?.memory_update;
  const changes = update?.status === "applied" ? update.changes.filter((change) => change.kind !== "Lens") : [];
  const waiting = rail.learningWaiting === "unknown" ? t("chatRail.learned.statusUnavailable") : rail.learningWaiting ? worldReadinessLabels()[rail.learningWaiting] : "";
  const openSaved = (change: MemoryUpdateChange) => {
    if (!go) return;
    if (change.kind === "principle") { go({ kind: "page", location: { loupistId, surface: "lens", recordId: change.recordId } }); return; }
    const location = knowledgeLocationFromMemoryChange(loupistId, change);
    if (location) go({ kind: "memory", location, title: change.title });
  };
  const counts = learned ? [
    learned.names ? t("chatRail.learned.names", { count: learned.names, formattedCount: formatNumber(learned.names) }) : "",
    learned.relationships ? t("chatRail.learned.relationships", { count: learned.relationships, formattedCount: formatNumber(learned.relationships) }) : "",
    learned.methods ? t("chatRail.learned.methods", { count: learned.methods, formattedCount: formatNumber(learned.methods) }) : "",
  ].filter(Boolean).join(" · ") : "";
  const empty = !learned && !changes.length && !digest && !rail.learningPending && !waiting;
  return <>
    {rail.learningPending ? <p className="activity-quiet" role="status"><i aria-hidden="true" />{t("chatRail.learned.processing")}</p>
      : waiting ? <p className="activity-quiet" role="status" data-waiting="">{waiting}</p> : null}
    {changes.length || digest ? <section className="activity-section" data-activity-section="saved">
      <h3><span>{t("activitySheet.learned.saved")}</span></h3>
      {changes.length ? <ul className="activity-saved">
        {changes.map((change) => {
          const title = change.kind === "principle" ? change.statement : change.title;
          return <li key={change.recordId} data-memory-change={`${change.operation}:${change.kind}`}>
            <small>{memoryChangeVerb(change)}</small>
            {go && change.kind !== "Journal" ? <button type="button" className="activity-link" onClick={() => openSaved(change)}>{title}</button> : <b>{title}</b>}
          </li>;
        })}
      </ul> : null}
      {digest ? <LearnedCard id={digestId} bridge={bridge} loupistId={loupistId} digest={digest} onChanged={onWorldChanged} /> : null}
    </section> : null}
    {learned ? <section className="activity-section" data-activity-section="map">
      <h3><span>{t("activitySheet.learned.map")}</span>{counts ? <span className="activity-section-meta">{counts}</span> : null}</h3>
      {learned.chips.length ? <div className="activity-chips">
        {learned.chips.map((chip) => <button type="button" key={chip.id} className="activity-chip" title={chip.label} disabled={!onShowOnMap} onClick={() => onShowOnMap?.(new Set([chip.id]))}>
          <i className={`map-swatch is-${chip.swatch}`} aria-hidden="true" /><span>{chip.label}</span>
        </button>)}
      </div> : null}
      {onShowOnMap ? <button type="button" className="activity-link activity-more" onClick={() => onShowOnMap(learned.highlightIds)}>
        {t("activitySheet.learned.openMap")}<ArrowUpRightIcon aria-hidden="true" />
      </button> : null}
    </section> : null}
    {empty ? <p className="activity-empty">{t("activitySheet.learned.none")}</p> : null}
  </>;
}

/** Brings the newest request that waits on the user into view in the answer and focuses its card. */
function showNeeds(data: RailData) {
  const requests = data.waiting.length || data.questions.length
    ? [...document.querySelectorAll<HTMLElement>(".approval-card[data-request-id], .question-card[data-request-id]")].filter((card) =>
      data.waiting.some((approval) => approval.requestId === card.dataset.requestId) || data.questions.some((question) => question.requestId === card.dataset.requestId))
    : [...document.querySelectorAll<HTMLElement>('.approval-line[data-state="settings"]')];
  const target = requests.at(-1);
  if (!target) return;
  const reduced = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  target.scrollIntoView?.({ block: "center", behavior: reduced ? "auto" : "smooth" });
  target.focus({ preventScroll: true });
}

/** What the waiting row names: the one request, or how many there are. */
function needsSummary(data: RailData) {
  const count = data.waiting.length + data.questions.length + data.needs.length;
  if (count > 1) return t("chatRail.needsYou.requests", { count, formattedCount: formatNumber(count) });
  if (data.questions[0]) return data.questions[0].question;
  if (data.waiting[0]) return humanizeToolName(data.waiting[0].toolName);
  return data.needs[0] ? humanizeToolName(data.needs[0].tool_name) : "";
}

function UsingPanel({ rail, onOpenUsing, onOpenFile, onShowOnMap }: {
  rail: RailData; onOpenUsing: (item: RailUsing) => void; onOpenFile: (path: string) => void; onShowOnMap?: (ids?: ReadonlySet<string>) => void;
}) {
  useTranslation("chat");
  const needs = rail.needs.length + rail.waiting.length + rail.questions.length;
  return <>
    {needs ? <section className="activity-section" data-activity-section="needs">
      <h3><span>{t("activitySheet.using.needs")}</span></h3>
      <button type="button" className="activity-needs" onClick={() => showNeeds(rail)}>
        <i aria-hidden="true" />
        <span className="activity-source-copy"><b>{t(rail.waiting.length + rail.needs.length === 0 ? "chatRail.needsYou.answerTitle" : "chatRail.needsYou.title")}</b><small>{needsSummary(rail)}</small></span>
        <span className="activity-needs-show">{t("chatRail.needsYou.show")}</span>
      </button>
    </section> : null}
    <section className="activity-section" data-activity-section="connections">
      <h3><span>{t("activitySheet.using.title")}</span></h3>
      {rail.using.length ? <ul className="activity-sources">
        {rail.using.map((item) => {
          const content = <span className="activity-source-copy"><b>{item.label}</b><small>{t(`chatRail.using.kind.${item.kind}`)}</small></span>;
          return <li key={item.key}>{item.kind === "connection"
            ? <div className="activity-source">{content}</div>
            : <button type="button" className="activity-source" onClick={() => onOpenUsing(item)}>{content}<ArrowUpRightIcon aria-hidden="true" /></button>}</li>;
        })}
      </ul> : <p className="activity-empty">{t("activitySheet.using.none")}</p>}
    </section>
    {/* Files get a heading only when there are files; the way to the chat's materials stays either way. */}
    {rail.files.length || onShowOnMap ? <section className="activity-section" data-activity-section="files">
      {rail.files.length ? <>
        <h3><span>{t("activitySheet.using.files")}</span><span className="activity-section-meta">{formatNumber(rail.files.length)}</span></h3>
        <ul className="activity-sources">{rail.files.map((file) => <FileRow key={file.key} file={file} onOpenFile={onOpenFile} />)}</ul>
      </> : null}
      {onShowOnMap ? <Button type="button" variant="outline" size="sm" className="activity-materials" data-activity-materials onClick={() => onShowOnMap()}>{t("activitySheet.using.materials")}</Button> : null}
    </section> : null}
  </>;
}

/** A sent image opens the chat's lightbox; a file the Loupist linked opens in the panel. */
function FileRow({ file, onOpenFile }: { file: RailFile; onOpenFile: (path: string) => void }) {
  useTranslation("chat");
  const lightbox = useChatLightbox();
  const Icon = file.kind === "image" ? ImageIcon : FileIcon;
  const meta = [file.sizeBytes !== undefined ? fileSize(file.sizeBytes) : "", file.fromLoupist ? t("chatRail.files.fromLoupist") : ""].filter(Boolean).join(" · ");
  const content = <><Icon aria-hidden="true" /><span className="activity-source-copy"><b title={file.path ?? file.filename}>{file.filename}</b>{meta ? <small>{meta}</small> : null}</span></>;
  const image = file.kind === "image" && file.messageId && file.attachmentId && lightbox;
  if (image) return <li><button type="button" className="activity-source" onClick={(event) => lightbox.open(file.messageId!, file.attachmentId!, event.currentTarget)}>{content}</button></li>;
  if (file.path) return <li><button type="button" className="activity-source" onClick={() => onOpenFile(file.path!)}>{content}</button></li>;
  return <li><div className="activity-source">{content}</div></li>;
}
