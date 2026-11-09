import i18n, { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { fileSize } from "../../chat/fileSize";
import { useEffect, useId, useState, type ReactNode } from "react";
import { ArrowUpRightIcon, ChevronDownIcon, ChevronRightIcon, FileIcon, LayersIcon, NetworkIcon, PaperclipIcon } from "lucide-react";
import { humanizeToolName } from "../connections/custom/toolTitle";
import type { LoupistBridge } from "../../types";
import type { RailData, RailFile, RailMode, RailUsing } from "../../chat/chatRail";
import { worldReadinessLabels } from "../../knowledge/worldReadiness";
import { Spinner } from "../ui/spinner";
import { useChatLightbox } from "./ChatLightbox";

const t = i18n.getFixedT(null, "chat");
const MAX_THUMBNAILS = 8;
const MAX_ROWS = 6;
const STORAGE_KEY = "loupist.chatRail.v1";

type CardId = "files" | "using" | "learned";
type Collapsed = Partial<Record<CardId, boolean>>;
/** Files start open; Using and Learned start folded (context-rail.md §2.1). */
const DEFAULT_COLLAPSED: Record<CardId, boolean> = { files: false, using: true, learned: true };

/** Per Loupist, per device: which cards the user folded. A convenience, not Memory. */
function readCollapsed(loupistId: string): Collapsed {
  try {
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Record<string, Collapsed>;
    return stored[loupistId] ?? {};
  } catch {
    return {};
  }
}
function writeCollapsed(loupistId: string, collapsed: Collapsed) {
  try {
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Record<string, Collapsed>;
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...stored, [loupistId]: collapsed }));
  } catch {
    // Private windows and blocked storage simply forget the choice.
  }
}

const sizeLabel = (size?: number) => size === undefined ? "" : fileSize(size);

export type ChatRailActions = {
  bridge?: LoupistBridge;
  loupistId: string;
  onOpenFile?: (path: string) => void;
  onOpenUsing: (item: RailUsing) => void;
  onShowLearned: (ids: ReadonlySet<string>) => void;
};

function Card({ id, title, summary, collapsed, onToggle, children }: { id: CardId; title: string; summary: ReactNode; collapsed: boolean; onToggle: () => void; children: ReactNode }) {
  const bodyId = `${useId()}-${id}`;
  const Icon = { files: PaperclipIcon, using: LayersIcon, learned: NetworkIcon }[id];
  const heading = <>
    <Icon aria-hidden="true" />
    <span className="chat-rail-card-title">{title}</span>
    <span className="chat-rail-card-summary">{summary}</span>
  </>;
  if (!children) return <section className="chat-rail-card" data-card={id}><div className="chat-rail-card-header">{heading}</div></section>;
  return <section className="chat-rail-card" data-card={id}>
    <button type="button" className="chat-rail-card-header" aria-expanded={!collapsed} aria-controls={bodyId} onClick={onToggle}>
      {heading}
      {collapsed ? <ChevronRightIcon aria-hidden="true" /> : <ChevronDownIcon aria-hidden="true" />}
    </button>
    <div id={bodyId} className="chat-rail-card-body" hidden={collapsed}>{children}</div>
  </section>;
}

/** A sent image as the chat's lightbox reads it. */
const railAttachment = (file: RailFile) => ({ id: file.attachmentId ?? "", filename: file.filename, media_type: "image/*", size_bytes: file.sizeBytes ?? 0, url: file.url });

/** Opens the chat's lightbox, which steps through every image in the chat. */
function Thumbnail({ file }: { file: RailFile }) {
  const lightbox = useChatLightbox();
  const messageId = file.messageId ?? "";
  const attachment = railAttachment(file);
  useEffect(() => {
    if (messageId && attachment.id) lightbox?.load(messageId, attachment);
    // Reads once per image; the lightbox keeps what it read.
  }, [lightbox?.load, messageId, attachment.id]);
  const { url, failed } = lightbox?.image(messageId, attachment) ?? { url: file.url, failed: false };
  return <button type="button" className="chat-rail-thumb" title={failed ? t("chatRail.previewUnavailable") : file.filename} aria-label={file.filename}
    disabled={!lightbox || !messageId || !attachment.id} onClick={(event) => lightbox?.open(messageId, attachment.id, event.currentTarget)}>
    {url ? <img src={url} alt="" /> : failed ? <FileIcon aria-hidden="true" /> : null}
  </button>;
}

/**
 * Brings the newest request that waits on the user into view in the answer and
 * moves focus to its card, never to one of its buttons.
 */
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

/** What the "Needs your OK" row names: the one request, or how many there are. */
function needsSummary(data: RailData) {
  const count = data.waiting.length + data.questions.length + data.needs.length;
  if (count > 1) return t("chatRail.needsYou.requests", { count, formattedCount: formatNumber(count) });
  if (data.questions[0]) return data.questions[0].question;
  const approval = data.waiting[0];
  if (approval) return humanizeToolName(approval.toolName);
  return data.needs[0] ? humanizeToolName(data.needs[0].tool_name) : "";
}

/** The same cards live beside a wide conversation or above a narrow one. */
export function ChatRailCards({ data, actions, compact = false }: { data: RailData; actions: ChatRailActions; compact?: boolean }) {
  useTranslation("chat");
  const [collapsed, setCollapsed] = useState<Collapsed>(() => readCollapsed(actions.loupistId));
  const [allFiles, setAllFiles] = useState(false);
  const [allUsing, setAllUsing] = useState(false);
  const lightbox = useChatLightbox();
  useEffect(() => { setCollapsed(readCollapsed(actions.loupistId)); }, [actions.loupistId]);
  const isCollapsed = (id: CardId) => collapsed[id] ?? DEFAULT_COLLAPSED[id];
  const toggle = (id: CardId) => {
    const next = { ...collapsed, [id]: !isCollapsed(id) };
    setCollapsed(next);
    writeCollapsed(actions.loupistId, next);
  };

  const images = data.files.filter((file) => file.kind === "image" && file.messageId);
  const otherFiles = data.files.filter((file) => !(file.kind === "image" && file.messageId));
  const shownImages = images.length > MAX_THUMBNAILS ? images.slice(0, MAX_THUMBNAILS - 1) : images;
  const shownFiles = allFiles ? otherFiles : otherFiles.slice(0, MAX_ROWS);
  const shownUsing = allUsing ? data.using : data.using.slice(0, MAX_ROWS);
  const counts = { principles: 0, methods: 0, connections: 0, pins: 0 };
  for (const item of data.using) {
    if (item.kind === "principle") counts.principles += 1;
    else if (item.kind === "method") counts.methods += 1;
    else if (item.kind === "connection") counts.connections += 1;
    else counts.pins += 1;
  }
  const usingSummary = [
    counts.principles ? t("chatRail.using.principles", { count: counts.principles, formattedCount: formatNumber(counts.principles) }) : "",
    counts.methods ? t("chatRail.using.methods", { count: counts.methods, formattedCount: formatNumber(counts.methods) }) : "",
    counts.connections ? t("chatRail.using.connections", { count: counts.connections, formattedCount: formatNumber(counts.connections) }) : "",
    counts.pins ? t("chatRail.using.pins", { count: counts.pins, formattedCount: formatNumber(counts.pins) }) : "",
  ].filter(Boolean).join(" · ");
  const learned = data.learned;
  const learnedSummary = learned ? [
    learned.names ? t("chatRail.learned.names", { count: learned.names, formattedCount: formatNumber(learned.names) }) : "",
    learned.relationships ? t("chatRail.learned.relationships", { count: learned.relationships, formattedCount: formatNumber(learned.relationships) }) : "",
    learned.methods ? t("chatRail.learned.methods", { count: learned.methods, formattedCount: formatNumber(learned.methods) }) : "",
  ].filter(Boolean).join(" · ") : "";

  const learningUnfinished = data.learningPending || !!data.learningWaiting;
  const waitingReason = data.learningWaiting === "unknown" ? t("chatRail.learned.statusUnavailable") : data.learningWaiting ? worldReadinessLabels()[data.learningWaiting] : "";
  const learnedCard = learned || learningUnfinished ? <Card key="learned" id="learned" title={t("chatRail.learned.title")} summary={data.learningPending
      ? <span className="chat-rail-processing" role="status"><Spinner className="motion-reduce:animate-none" aria-hidden="true" />{t("chatRail.learned.processing")}</span>
      : data.learningWaiting ? <span role="status">{t("chatRail.learned.waiting", { reason: waitingReason })}</span> : learnedSummary} collapsed={isCollapsed("learned")} onToggle={() => toggle("learned")}>
      {learned && <>
        {learned.chips.length ? <div className="chat-rail-chips">
          {learned.chips.map((chip) => <button type="button" key={chip.id} className="chat-rail-chip" title={chip.label} onClick={() => actions.onShowLearned(new Set([chip.id]))}>
            <i className={`map-swatch is-${chip.swatch}`} aria-hidden="true" /><span>{chip.label}</span>
          </button>)}
        </div> : null}
        <button type="button" className="chat-rail-map-link" onClick={() => actions.onShowLearned(learned.highlightIds)}>
          {i18n.t("knowledge:memoryPage.history.openMap")}<ArrowUpRightIcon aria-hidden="true" />
        </button>
      </>}
    </Card> : null;
  // Keep requests and source-review status visible before an open Files card in compact space.
  const needsCard = data.needs.length || data.waiting.length || data.questions.length ? <section key="needs" className="chat-rail-card chat-rail-needs" data-card="needs">
      <button type="button" className="chat-rail-needs-row" onClick={() => showNeeds(data)}>
        <span className="chat-rail-needs-dot" aria-hidden="true" />
        <span className="chat-rail-needs-title">{t(data.waiting.length + data.needs.length === 0 ? "chatRail.needsYou.answerTitle" : "chatRail.needsYou.title")}</span>
        <span className="chat-rail-card-summary">{needsSummary(data)}</span>
        <span className="chat-rail-needs-show">{t("chatRail.needsYou.show")}</span>
      </button>
    </section> : null;

  return <>
    {compact && needsCard}
    {compact && learningUnfinished && learnedCard}
    {data.files.length ? <Card id="files" title={t("chatRail.files.title")} summary={[
      images.length ? t("chatRail.files.images", { count: images.length, formattedCount: formatNumber(images.length) }) : "",
      otherFiles.length ? t("chatRail.files.files", { count: otherFiles.length, formattedCount: formatNumber(otherFiles.length) }) : "",
    ].filter(Boolean).join(" · ")} collapsed={isCollapsed("files")} onToggle={() => toggle("files")}>
      {images.length ? <div className="chat-rail-thumbs">
        {shownImages.map((file) => <Thumbnail key={file.key} file={file} />)}
        {/* "+n" opens the lightbox at the first image it stands for (§2.1). */}
        {images.length > MAX_THUMBNAILS ? <button type="button" className="chat-rail-thumb-more" disabled={!lightbox}
          onClick={(event) => { const first = images[shownImages.length]; if (first?.messageId && first.attachmentId) lightbox?.open(first.messageId, first.attachmentId, event.currentTarget); }}>
          {t("chatRail.files.more", { formattedCount: formatNumber(images.length - shownImages.length) })}
        </button> : null}
      </div> : null}
      {shownFiles.length ? <ul className="chat-rail-rows">
        {shownFiles.map((file) => <li key={file.key}>
          <button type="button" className="chat-rail-row" disabled={!file.path || !actions.onOpenFile} onClick={() => file.path && actions.onOpenFile?.(file.path)} title={file.path ?? file.filename}>
            <FileIcon aria-hidden="true" />
            <span className="chat-rail-row-label">{file.filename}</span>
            {file.sizeBytes !== undefined ? <small>{sizeLabel(file.sizeBytes)}</small> : null}
            {file.fromLoupist ? <small className="chat-rail-from-loupist">{t("chatRail.files.fromLoupist")}</small> : null}
          </button>
        </li>)}
      </ul> : null}
      {!allFiles && otherFiles.length > MAX_ROWS ? <button type="button" className="chat-rail-more" onClick={() => setAllFiles(true)}>{t("chatRail.showAll")}</button> : null}
    </Card> : null}
    {data.using.length ? <Card id="using" title={t("chatRail.using.title")} summary={usingSummary} collapsed={isCollapsed("using")} onToggle={() => toggle("using")}>
      <ul className="chat-rail-rows">
        {shownUsing.map((item) => {
          const row = <>
            <span className="chat-rail-row-label">{item.label}</span>
            <span className="chat-rail-row-kind">{t(`chatRail.using.kind.${item.kind}`)}</span>
          </>;
          return <li key={item.key}>{item.kind === "connection"
            ? <div className="chat-rail-row">{row}</div>
            : <button type="button" className="chat-rail-row" onClick={() => actions.onOpenUsing(item)}>{row}</button>}
          </li>;
        })}
      </ul>
      {!allUsing && data.using.length > MAX_ROWS ? <button type="button" className="chat-rail-more" onClick={() => setAllUsing(true)}>{t("chatRail.showAll")}</button> : null}
    </Card> : null}
    {(!compact || !learningUnfinished) && learnedCard}
    {!compact && needsCard}
  </>;
}

/** Context stays in the chat body, beside the conversation or in its own compact row. */
export function ChatRail({ mode, data, actions }: { mode: RailMode; data: RailData; actions: ChatRailActions }) {
  useTranslation("chat");
  if (mode === "none") return null;
  return <aside className="chat-rail" data-placement={mode} aria-label={t("chatRail.label")}><ChatRailCards data={data} actions={actions} compact={mode === "inline"} /></aside>;
}
