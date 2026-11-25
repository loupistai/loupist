import i18n, { useTranslation } from "@/i18n";
import type { ChatDraft } from "../../app/useChatDrafts";
import {
  ClipboardIcon,
  EyeOffIcon,
  ArrowUpIcon,
  CornerDownRightIcon,
  FileIcon,
  LibraryIcon,
  MapIcon,
  PlusIcon,
  SquareIcon,
  XIcon,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type KeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import { useInputModality } from "@/lib/inputModality";
import { useExtensionAvailability } from "@/app/extensionAvailability";
import { skillBlurb, skillPresentation, visibleSkill } from "@/lib/agentSkills";
import { formatNumber } from "@/i18n/format";
import { composeSubmission, composerChipInlineText, contextGroupLabel, rankText, type ComposerChip, type ContextOption } from "../../chat/composerChips";
import type { FileUIPart } from "ai";
import type { ChatArtifactRef, KnowledgePin, LoupistBridge } from "../../types";
import {
  PromptInput,
  PromptInputActionAddAttachments,
  PromptInputActionMenu,
  PromptInputActionMenuContent,
  PromptInputActionMenuItem,
  PromptInputActionMenuTrigger,
  PromptInputBody,
  PromptInputButton,
  PromptInputFooter,
  PromptInputTextarea,
  PromptInputTools,
  pastedImage,
  usePromptInputAttachments,
} from "@/components/ai-elements/prompt-input";
import { InputGroupButton } from "@/components/ui/input-group";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { Alert, AlertDescription } from "@/components/ui/alert";
import type {
  ConfiguredModel,
  ModelProviderOption,
  ModelRunSelection,
} from "../../types";
import { cn } from "@/lib/utils";
import { ChatModelMenu } from "./ChatModelMenu";
import { ComposerChipTray, ComposerInlineReferenceControls } from "./ComposerChipTray";
import { useComposerInlineReferences, type InlineComposerItem } from "./useComposerInlineReferences";
import { knowledgePinKey } from "../../knowledge/location";
import { activeTriggerAt, ComposerAssistMenu, ComposerPromptHighlight, type ActiveTrigger, type AssistOption, type ComposerOption, type ComposerSkill } from "./ComposerAssist";
import { AsOfDatePanel } from "./ComposerAsOf";
import { isMacPlatform, shortcutFor } from "../../app/shortcuts";
import { asOfProblem, formatAsOf, typedAsOf } from "../../chat/asOf";
import { fileSize } from "../../chat/fileSize";
import {
  DropdownMenuLabel,
  DropdownMenuCheckboxItem,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu";

const t = i18n.getFixedT(null, "chat");
/** Space between the composer's top edge and a menu that opens above it (`.composer-assist`). */
const COMPOSER_MENU_GAP = 10;
const MEMORY_SKILL_IDS: readonly string[] = ["world", "lens", "journal"];
type ReadClipboardImage = LoupistBridge["clipboardReadImage"];

const validateAttachment = (file: File) =>
  file.type.startsWith("image/") && file.size > 5 * 1024 * 1024
    ? t("chatComposer.imageAttachmentsMustBe5MBOrSmaller")
    : undefined;


type Props = {
  attachmentDraft?: ChatDraft;
  inputLabel?: string;
  placeholder?: string;
  prompt: string;
  privateChat: boolean;
  isRunning: boolean;
  isSaving?: boolean;
  isStopping?: boolean;
  error: string | null;
  sendDisabledReason?: string;
  streamAnnouncement: string;
  models: ConfiguredModel[];
  providers: ModelProviderOption[];
  modelSelection: ModelRunSelection;
  skills: ComposerSkill[];
  plugins: ComposerOption[];
  knowledgePins?: readonly KnowledgePin[];
  chips?: readonly ComposerChip[];
  selectedArtifact?: ChatArtifactRef;
  onClearSelectedArtifact?: () => void;
  onRemoveKnowledgePin?: (pin: KnowledgePin) => void;
  onChipsChange?: (chips: ComposerChip[]) => void;
  onSearchContext?: (query: string) => Promise<ContextOption[]>;
  onAddKnowledgePin?: (pin: KnowledgePin) => void;
  /** "Screenshot of World" in the + menu, available when the conversation has a World view. */
  mapScreenshot?: MapScreenshot;
  readClipboardImage?: ReadClipboardImage;
  /** The chat's saved Sources, offered as "Add from Evidence". */
  evidenceSources?: readonly EvidenceSourceOption[];
  onSelectArtifact?: (artifact: ChatArtifactRef) => void;
  onPromptChange: (prompt: string) => void;
  onPrivateChatChange: (enabled: boolean) => void;
  onModelSelectionChange: (selection: ModelRunSelection) => void;
  /** The composer shows the host’s default model until a choice is saved. */
  modelIsDefault: boolean;
  modelName?: string;
  noAccount?: boolean;
  onChooseDefault: () => void;
  onSend: (prompt: string, files: FileUIPart[], draftText?: string) => boolean | void | Promise<boolean | void>;
  onFollowUp: (text?: string, files?: FileUIPart[], draftText?: string) => boolean | void | Promise<boolean | void>;
  onAbort: () => void;
  onOpenModelSettings?: () => void;
  /** Rust opens the provider's usage page from the menu's plan line. */
  onOpenUsage?: (provider: string) => void;
  /** Opens the run-profile menu from outside (the default-changed line's Change). */
  openModelMenuSignal?: number;
  /** The next message's as-of date (`YYYY-MM-DD`); it stays for follow-ups until removed. */
  asOf?: string;
  onAsOfChange?: (asOf: string | undefined) => void;
  /** Files to attach from outside the composer (the lightbox's "Add to question"); never sends. */
  incomingFiles?: IncomingFiles;
  /** The composer took `incomingFiles` into its tray (or refused them with its usual alert). */
  onIncomingFilesTaken?: () => void;
  /** A layer over the conversation (the overlaying activity sheet) is open: Esc closes it before it stops a reply. */
  layerOpen?: boolean;
};

export type IncomingFiles = { id: number; files: File[] };

/**
 * Sizes the message to its text; the CSS max-height caps it and the rest scrolls.
 * `field-sizing: content` is missing from the WebKit of supported macOS releases,
 * where a one-row textarea stayed one row high and cut off a second line.
 * The scroll position is kept, so the line the browser scrolled to (the caret's) stays in view;
 * with the caret at the end, the message scrolls to its last line.
 */
function fitPromptHeight(textarea: HTMLTextAreaElement) {
  // An empty editor is one row. WebKit can still report the old text extent
  // after the first send changes the docked layout and clears the textarea.
  if (!textarea.value) {
    textarea.style.height = "";
    textarea.scrollTop = 0;
    return;
  }
  const { scrollTop } = textarea;
  // Measured without a scrollbar, so a classic one cannot add a line that is not there.
  textarea.style.overflowY = "hidden";
  textarea.style.height = "auto";
  const style = getComputedStyle(textarea);
  const borders = (parseFloat(style.borderTopWidth) || 0) + (parseFloat(style.borderBottomWidth) || 0);
  textarea.style.height = `${textarea.scrollHeight + borders}px`;
  textarea.style.overflowY = "";
  // Typing at the end keeps the last line in view even where WebKit has not yet scrolled
  // to the caret when the height changes; elsewhere the browser's own scroll position stays.
  const atEnd = textarea.selectionStart === textarea.value.length && textarea.selectionEnd === textarea.value.length;
  textarea.scrollTop = atEnd ? textarea.scrollHeight : scrollTop;
}

/** Attaches files handed in from outside through the tray's own checks: size, count and type. */
function ComposerIncomingFiles({ request, onTaken }: { request?: IncomingFiles; onTaken?: () => void }) {
  const attachments = usePromptInputAttachments();
  const taken = useRef<number | null>(null);
  useEffect(() => {
    if (!request || taken.current === request.id) return;
    taken.current = request.id;
    attachments.add(request.files);
    onTaken?.();
  }, [attachments, onTaken, request]);
  return null;
}

/** The files attached to the next message; a file still being read counts, and the tray says so until it is in. */
function ComposerAttachmentTray({ onFilesChange }: { onFilesChange: (count: number) => void}) {
  useTranslation("chat");
  const attachments = usePromptInputAttachments();
  const { reading } = attachments;
  useEffect(() => {
    onFilesChange(attachments.files.length + reading);
  }, [attachments.files, reading, onFilesChange]);
  if (attachments.files.length === 0 && reading === 0) return null;
  const count = attachments.files.length;
  const readingLabel = t("chatComposer.attachments.reading", { count: reading, formattedCount: formatNumber(reading) });
  const name = (filename?: string) => filename ?? t("chatComposer.attachmentName");
  // The size of the file the user picked, in the one format files use everywhere ("1.2 MB").
  const size = (bytes?: number) => bytes === undefined ? "" : fileSize(bytes);
  const titled = (file: { filename?: string; sizeBytes?: number }) => [name(file.filename), size(file.sizeBytes)].filter(Boolean).join(" · ");
  // The × goes away with its file, so focus returns to the message rather than to the page.
  const remove = (id: string, event: { currentTarget: HTMLElement }) => {
    const message = event.currentTarget.closest("form")?.querySelector("textarea");
    attachments.remove(id);
    message?.focus();
  };
  return (
    <div className="composer-attachments" role="group" aria-label={count ? t("chatComposer.attachments.count", { count, formattedCount: formatNumber(count) }) : readingLabel}>
      {attachments.files.map((file) => file.mediaType.startsWith("image/") ? (
        <figure className="composer-thumb" key={file.id} title={titled(file)}>
          <img src={file.url} alt={name(file.filename)} />
          <button type="button" aria-label={t("chatComposer.removeAttachment", { value1: name(file.filename) })} onClick={(event) => remove(file.id, event)}>
            <XIcon aria-hidden="true" />
          </button>
        </figure>
      ) : (
        <span className="composer-file-chip" key={file.id} title={titled(file)}>
          <FileIcon aria-hidden="true" />
          <span className="composer-file-name">{name(file.filename)}</span>
          {file.sizeBytes === undefined ? null : <span className="composer-file-size gg-num">{size(file.sizeBytes)}</span>}
          <button type="button" aria-label={t("chatComposer.removeAttachment", { value1: name(file.filename) })} onClick={(event) => remove(file.id, event)}>
            <XIcon aria-hidden="true" />
          </button>
        </span>
      ))}
      <span className="composer-attachments-meta">
        {reading ? <span className="composer-attachments-reading" role="status"><Spinner aria-hidden="true" />{readingLabel}</span> : null}
        {count ? t("chatComposer.attachments.count", { count, formattedCount: formatNumber(count) }) : null}
        {count >= 2 ? <button type="button" onClick={(event) => { const message = event.currentTarget.closest("form")?.querySelector("textarea"); for (const file of [...attachments.files]) attachments.remove(file.id); message?.focus(); }}>{t("chatComposer.attachments.clearAll")}</button> : null}
      </span>
    </div>
  );
}

/**
 * While files are dragged over the chat column, one overlay names the drop and
 * its limits; dropping anywhere in the column attaches (context-rail.md R2).
 */
function ComposerDropOverlay() {
  useTranslation("chat");
  const attachments = usePromptInputAttachments();
  const anchor = useRef<HTMLSpanElement>(null);
  const [column, setColumn] = useState<HTMLElement | null>(null);
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    const element = anchor.current?.closest<HTMLElement>(".chat-workspace") ?? null;
    setColumn(element);
    if (!element) return undefined;
    let depth = 0;
    const hasFiles = (event: DragEvent) => Boolean(event.dataTransfer?.types?.includes("Files"));
    const enter = (event: DragEvent) => { if (!hasFiles(event)) return; depth += 1; setDragging(true); };
    const over = (event: DragEvent) => { if (hasFiles(event)) event.preventDefault(); };
    const leave = (event: DragEvent) => { if (!hasFiles(event)) return; depth = Math.max(0, depth - 1); if (!depth) setDragging(false); };
    const drop = (event: DragEvent) => {
      if (!hasFiles(event)) return;
      event.preventDefault();
      depth = 0;
      setDragging(false);
      // The composer form handles drops on itself; everything else in the column lands here.
      if ((event.target as Element | null)?.closest?.("form")) return;
      if (event.dataTransfer?.files.length) attachments.add(event.dataTransfer.files);
    };
    element.addEventListener("dragenter", enter);
    element.addEventListener("dragover", over);
    element.addEventListener("dragleave", leave);
    element.addEventListener("drop", drop);
    return () => {
      element.removeEventListener("dragenter", enter);
      element.removeEventListener("dragover", over);
      element.removeEventListener("dragleave", leave);
      element.removeEventListener("drop", drop);
    };
  }, [attachments]);
  return <>
    <span ref={anchor} hidden />
    {dragging && column ? createPortal(
      <div className="composer-drop-overlay" aria-live="polite">
        <p className="composer-drop-title">{t("chatComposer.drop.title")}</p>
        <p className="composer-drop-limits">{t("chatComposer.drop.limits")}</p>
      </div>,
      column,
    ) : null}
  </>;
}

export type MapScreenshot = {
  /** Read when the menu opens: whether a drawn map is there to capture. */
  status: () => "ready" | "empty" | "closed";
  take: () => Promise<File | null>;
};
export type EvidenceSourceOption = { key: string; label: string; pin: KnowledgePin };

/** Attaches a PNG of the map the user is looking at, with the export footer (context-rail.md X14). */
function MapScreenshotAction({ mapScreenshot, onError }: { mapScreenshot: MapScreenshot; onError: (message: string | null) => void }) {
  useTranslation("chat");
  const attachments = usePromptInputAttachments();
  const status = mapScreenshot.status();
  const reason = status === "empty" ? t("chatComposer.attach.mapEmpty") : status === "closed" ? t("chatComposer.attach.mapClosed") : undefined;
  return (
    <PromptInputActionMenuItem
      disabled={status !== "ready"}
      title={reason}
      onSelect={(event) => {
        event.preventDefault();
        onError(null);
        void mapScreenshot.take().then((file) => {
          if (file) attachments.add([file]);
          else onError(t("chatComposer.attach.mapFailed"));
        }).catch(() => onError(t("chatComposer.attach.mapFailed")));
      }}
    >
      {/* Why it is unavailable is the row's tooltip, so the label never wraps. */}
      <MapIcon />{t("chatComposer.attach.mapScreenshot")}
    </PromptInputActionMenuItem>
  );
}

function ClipboardAttachmentAction({
  onError,
  readClipboardImage,
}: {
  onError: (message: string | null) => void;
  readClipboardImage?: ReadClipboardImage;
}) {
  useTranslation("chat");
  const attachments = usePromptInputAttachments();

  const attachClipboard = async () => {
    onError(null);
    try {
      const image = await readClipboardImage?.();
      switch (image?.status) {
        case "image": {
          const bytes = Uint8Array.from(atob(image.dataBase64), (byte) => byte.charCodeAt(0));
          attachments.add([pastedImage(new Blob([bytes], { type: "image/png" }))]);
          break;
        }
        case "noImage":
          onError(t("chatComposer.theClipboardDoesNotContainAnAttachableImage"));
          break;
        case "tooLarge":
          onError(t("chatComposer.imageAttachmentsMustBe5MBOrSmaller"));
          break;
        default:
          onError(t("chatComposer.clipboardNotRead", { paste: isMacPlatform() ? "⌘V" : "Ctrl+V" }));
      }
    } catch {
      onError(t("chatComposer.couldNotAttachClipboardContents"));
    }
  };

  return (
    <PromptInputActionMenuItem onSelect={() => { void attachClipboard(); }}>
      <ClipboardIcon />{t("chatComposer.attach.pasteImage")}</PromptInputActionMenuItem>
  );
}

/** The + menu at the composer's leading edge: files, the clipboard, the map, saved Sources and private chat. */
function ComposerAddMenu({
  privateChat,
  onPrivateChatChange,
  onAttachmentError,
  mapScreenshot,
  readClipboardImage,
  evidenceSources,
  onAddKnowledgePin,
}: {
  privateChat: boolean;
  onPrivateChatChange: (enabled: boolean) => void;
  onAttachmentError: (message: string | null) => void;
  mapScreenshot?: MapScreenshot;
  readClipboardImage?: ReadClipboardImage;
  evidenceSources?: readonly EvidenceSourceOption[];
  onAddKnowledgePin?: (pin: KnowledgePin) => void;
}) {
  useTranslation("chat");
  const addTrigger = useRef<HTMLButtonElement>(null);
  const [addMenuOffset, setAddMenuOffset] = useState({ side: 4, align: 0 });
  // The + menu shows its focus ring for keys only (WebKit would ring it after a click from the composer).
  const modality = useInputModality();
  // Radix places a menu against its trigger. The + menu opens from the whole composer instead, like the
  // `@` and `/` menus: left edges aligned, just above the composer's top edge.
  const anchorAddMenu = (open: boolean) => {
    const trigger = addTrigger.current;
    const composer = trigger?.closest<HTMLElement>('[data-slot="input-group"]');
    if (!open || !trigger || !composer) return;
    const from = trigger.getBoundingClientRect(), to = composer.getBoundingClientRect();
    setAddMenuOffset({ side: from.top - to.top + COMPOSER_MENU_GAP, align: to.left - from.left });
  };
  return (
      <PromptInputActionMenu onOpenChange={anchorAddMenu}>
        <PromptInputActionMenuTrigger
          ref={addTrigger}
          className="composer-add-trigger"
          aria-label={t("chatComposer.addToThisMessage")}
          tooltip={t("chatComposer.addToThisMessage")}
          data-input={modality}
        >
          <PlusIcon />
        </PromptInputActionMenuTrigger>
        {/* Closing returns to the message, not the "+" button: its tooltip would cover the Private chip just turned on. */}
        <PromptInputActionMenuContent className="composer-add-menu" data-input={modality} side="top" align="start" sideOffset={addMenuOffset.side} alignOffset={addMenuOffset.align}
          onCloseAutoFocus={(event) => {
            const textarea = addTrigger.current?.closest("form")?.querySelector("textarea");
            if (!textarea) return;
            event.preventDefault();
            textarea.focus();
          }}>
          <DropdownMenuLabel>{t("chatComposer.addToThisMessage")}</DropdownMenuLabel>
          <PromptInputActionAddAttachments label={t("chatComposer.attach.files")} />
          <ClipboardAttachmentAction onError={onAttachmentError} readClipboardImage={readClipboardImage} />
          {mapScreenshot ? <MapScreenshotAction mapScreenshot={mapScreenshot} onError={onAttachmentError} /> : null}
          {evidenceSources?.length && onAddKnowledgePin ? <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel>{t("chatComposer.attach.fromEvidence")}</DropdownMenuLabel>
            {evidenceSources.map((source) => <PromptInputActionMenuItem key={source.key} title={source.label} onSelect={() => onAddKnowledgePin(source.pin)}>
              <LibraryIcon /><span className="composer-add-menu-label">{source.label}</span>
            </PromptInputActionMenuItem>)}
          </> : null}
          <DropdownMenuSeparator />
          <DropdownMenuCheckboxItem className="composer-private-item" data-composer-private checked={privateChat} onCheckedChange={(checked) => onPrivateChatChange(checked === true)}>
            <EyeOffIcon aria-hidden="true" />
            <span className="composer-private-item-copy"><span>{t("chatComposer.privateChat")}</span><small>{t("chatComposer.privateMenuDetail")}</small></span>
          </DropdownMenuCheckboxItem>
        </PromptInputActionMenuContent>
      </PromptInputActionMenu>
  );
}

/**
 * Guide and Send next, offered beside the tile only while a reply runs and the
 * composer has text. Send next is then the primary control and the tile's Stop
 * steps back to an outline.
 */
function ComposerRunExtras({
  showSend,
  showGuide,
  canSteer,
  guidePending,
  onQueue,
  onSteer,
  onPreserveFocus,
}: {
  showSend: boolean;
  showGuide: boolean;
  canSteer: boolean;
  guidePending: boolean;
  onQueue: () => void;
  onSteer: () => void;
  onPreserveFocus: (event: { preventDefault: () => void }) => void;
}) {
  useTranslation("chat");
  return (
    <>
      {showGuide ? (
        <PromptInputButton
          aria-label={t("chatComposer.guideThisResponse")}
          aria-busy={guidePending || undefined}
          className="composer-steer-action"
          disabled={!canSteer}
          tooltip={{
            content: canSteer
              ? t("chatComposer.guideThisResponseDetail")
              : t("chatComposer.useSendNextToIncludeAttachedFilesOrReferences"),
            shortcut: canSteer ? shortcutFor("guide") : undefined,
          }}
          variant="ghost"
          onMouseDown={onPreserveFocus}
          onClick={onSteer}
        >
          <CornerDownRightIcon />
        </PromptInputButton>
      ) : null}
      {showSend ? (
        <PromptInputButton
          aria-label={t("chatComposer.sendNext")}
          className="composer-queue-action"
          tooltip={{
            content: t("chatComposer.sendNextDetail"),
            shortcut: shortcutFor("send"),
          }}
          variant="default"
          onMouseDown={onPreserveFocus}
          onClick={onQueue}
        >
          <ArrowUpIcon />
        </PromptInputButton>
      ) : null}
    </>
  );
}

/**
 * Send and Stop are one round accent tile at the composer's trailing edge. The same button stays mounted from idle to running and back: its two
 * icons cross-fade in place (120 ms) and its box never changes. Pressing it
 * leaves focus in the textarea. After Stop it dims and ignores presses until
 * the host confirms.
 */
function ComposerTile({
  running,
  saving,
  stopping,
  prompt,
  hasModel,
  disabledReason,
  onAbort,
  onPreserveFocus,
}: {
  running: boolean;
  saving: boolean;
  /** Stop was pressed and the host has not confirmed it. */
  stopping: boolean;
  prompt: string;
  hasModel: boolean;
  disabledReason?: string;
  onAbort: () => void;
  onPreserveFocus: (event: { preventDefault: () => void }) => void;
}) {
  useTranslation("chat");
  const attachments = usePromptInputAttachments();
  const [tooltip, setTooltip] = useState(false);
  const submitting = !running && attachments.isSubmitting;
  // A file still being read makes the message sendable: Send waits for it.
  const empty = !prompt.trim() && attachments.files.length === 0 && attachments.reading === 0;
  const label = running ? saving ? t("memorySaving.skip") : t("chatComposer.stopResponse") : t("chatComposer.send");
  return (
    <Tooltip open={running && tooltip} onOpenChange={setTooltip}>
      <TooltipTrigger asChild>
        <InputGroupButton
          className={cn("composer-tile", running ? "composer-stop-action" : "composer-submit")}
          data-state={running ? "stop" : "send"}
          aria-label={label}
          size="icon-sm"
          variant="default"
          type={running ? "button" : "submit"}
          disabled={!running && (empty || !hasModel || submitting)}
          aria-disabled={running && stopping ? true : undefined}
          title={!running && !hasModel ? disabledReason : undefined}
          onMouseDown={onPreserveFocus}
          onClick={running ? (event) => {
            // Stop turns the tile back into Send, which an empty message disables: the message keeps focus.
            event.currentTarget.closest("form")?.querySelector("textarea")?.focus();
            onAbort();
          } : undefined}
        >
          <span className="composer-tile-icon" data-icon="send" aria-hidden="true">{submitting ? <Spinner /> : <ArrowUpIcon />}</span>
          <span className="composer-tile-icon" data-icon="stop" aria-hidden="true"><SquareIcon /></span>
        </InputGroupButton>
      </TooltipTrigger>
      <TooltipContent side="top">
        {saving ? t("memorySaving.skipDetail") : label}
        <span className="ml-2 text-muted-foreground">{shortcutFor("stop")}</span>
      </TooltipContent>
    </Tooltip>
  );
}

/** Any AI account can run a model: without one, the composer asks to connect instead of offering Send. */
const hasAccount = (models: readonly ConfiguredModel[], providers: readonly ModelProviderOption[]) =>
  providers.some((provider) => provider.credential_source !== "missing") || models.some((model) => model.credential_source !== "missing");

export function ChatComposer({
  attachmentDraft,
  inputLabel = t("chatComposer.messageLoupist"),
  placeholder = t("chatComposer.askLoupistToAddContextForCommands", { loupistName: "Loupist" }),
  prompt,
  privateChat,
  isRunning,
  isStopping = false,
  isSaving = false,
  error,
  sendDisabledReason,
  streamAnnouncement,
  models,
  providers,
  modelSelection,
  skills,
  plugins,
  knowledgePins = [],
  chips,
  selectedArtifact,
  onClearSelectedArtifact,
  onRemoveKnowledgePin,
  onChipsChange,
  onSearchContext,
  onAddKnowledgePin,
  mapScreenshot,
  readClipboardImage,
  evidenceSources,
  onSelectArtifact,
  onPromptChange,
  onPrivateChatChange,
  onModelSelectionChange,
  modelIsDefault,
  modelName,
  noAccount,
  onChooseDefault,
  onSend,
  onFollowUp,
  onAbort,
  onOpenModelSettings,
  onOpenUsage,
  openModelMenuSignal = 0,
  asOf,
  onAsOfChange,
  incomingFiles,
  onIncomingFilesTaken,
  layerOpen = false,
}: Props) {
  useTranslation("chat");
  const { userSkills } = useExtensionAvailability();
  const availableSkills = useMemo(() => skills.filter(skill => visibleSkill(skill, userSkills)), [skills, userSkills]);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const [attachmentCount, setAttachmentCount] = useState(0);
  const hasAttachedContext = attachmentCount > 0 || knowledgePins.length > 0 || Boolean(selectedArtifact) || Boolean(asOf);
  const onFilesChange = useCallback((count: number) => {
    setAttachmentCount(count);
  }, []);
  const [activeTrigger, setActiveTrigger] = useState<ActiveTrigger | null>(
    null,
  );
  const [activeOption, setActiveOption] = useState(0);
  const [contextOptions, setContextOptions] = useState<ContextOption[]>([]);
  const searchContextRef = useRef(onSearchContext);
  searchContextRef.current = onSearchContext;
  const canSearchContext = Boolean(onSearchContext);
  const [localChips, setOwnedChips] = useState<ComposerChip[]>(() => [...(chips ?? [])]);
  // Controlled draft chips are read in the same render as pins/ranges on native restoration.
  const ownedChips = chips ?? localChips;
  const ownedChipsRef = useRef(ownedChips);
  ownedChipsRef.current = ownedChips;
  const updateChips = (next: ComposerChip[]) => {
    ownedChipsRef.current = next;
    setOwnedChips(next);
    onChipsChange?.(next);
  };
  const [isComposing, setIsComposing] = useState(false);
  /** The `/asof` date panel in the assist menu's frame; `trigger` restores the menu on Escape. */
  const [datePanel, setDatePanel] = useState<{ trigger: ActiveTrigger | null; initial?: string } | null>(null);
  const assistId = useId();
  const runHelpId = useId();
  const assistRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const highlightRef = useRef<HTMLDivElement>(null);
  const chipItem = (chip: ComposerChip): InlineComposerItem => ({ key: `chip:${chip.key}`, kind: chip.kind, text: composerChipInlineText(chip), label: chip.label,
    onRemove: () => updateChips(ownedChipsRef.current.filter(existing => existing.key !== chip.key)),
    onRestore: () => updateChips([...ownedChipsRef.current.filter(existing => existing.key !== chip.key), chip]) });
  const pinItem = (pin: KnowledgePin): InlineComposerItem => ({ key: `pin:${knowledgePinKey(pin)}`, kind: pin.kind === "source" && pin.spanId ? "source_span" : pin.kind, text: pin.selection?.text ? `“${pin.selection.text}”` : `⟦${pin.label}⟧`, label: pin.label,
    onRemove: () => onRemoveKnowledgePin?.(pin), onRestore: () => onAddKnowledgePin?.(pin) });
  const artifactItem = (artifact: ChatArtifactRef): InlineComposerItem => ({ key: `artifact:${artifact.artifact_id}:${artifact.revision}`, kind: "artifact", text: `⟦${artifact.title}⟧`, label: artifact.title,
    onRemove: () => onClearSelectedArtifact?.(), onRestore: () => onSelectArtifact?.(artifact) });
  const inlineItems = [...ownedChips.filter(chip => chip.kind !== "skill").map(chipItem), ...knowledgePins.map(pinItem), ...(selectedArtifact ? [artifactItem(selectedArtifact)] : [])];
  const inline = useComposerInlineReferences({ prompt, onPromptChange, items: inlineItems, draft: attachmentDraft, textarea: textareaRef });

  const steering = useRef(false);
  const admittedTimer = useRef<number | null>(null);
  const [guidance, setGuidance] = useState<{ text: string; phase: "pending" | "admitted" } | null>(null);
  useEffect(() => () => {
    if (admittedTimer.current !== null) window.clearTimeout(admittedTimer.current);
  }, []);
  const selectedModel = models.find(
    (model) => model.id === modelSelection.model_profile_id,
  );
  const accountConnected = noAccount === undefined ? hasAccount(models, providers) : !noAccount;
  const canSetAsOf = Boolean(onAsOfChange);
  const assistOptions = useMemo<AssistOption[]>(() => {
    if (!activeTrigger) return [];
    const query = activeTrigger.query;
    if (activeTrigger.symbol === "@") {
      return contextOptions.map((option) => ({
        id: option.id,
        kind: "context" as const,
        label: option.label,
        name: option.label,
        detail: option.detail,
        group: contextGroupLabel(option.group),
        context: option,
      }));
    }
    const memory: AssistOption[] = [];
    const otherSkills: AssistOption[] = [];
    for (const skill of availableSkills) {
      const bundled = skill.source === "bundled";
      const memoryIndex = bundled ? MEMORY_SKILL_IDS.indexOf(skill.id) : -1;
      if (bundled && memoryIndex < 0 && !query.trim()) continue;
      const { name, description } = skillPresentation(skill);
      const detail = skillBlurb(skill);
      if (!rankText(query, [name, skill.name, skill.id, description, skill.description, detail])) continue;
      const option: AssistOption = {
        id: `skill:${skill.id}`, kind: "skill", name, label: name, detail,
        group: memoryIndex < 0 ? t("chatComposer.skills") : t("chatComposer.memory"),
      };
      if (memoryIndex >= 0) memory[memoryIndex] = option;
      else otherSkills.push(option);
    }
    otherSkills.sort((a, b) => rankText(query, [b.name]) - rankText(query, [a.name]) || a.name.localeCompare(b.name, i18n.language));
    const options = memory.filter(Boolean).concat(otherSkills);
    const asof: AssistOption = {
      id: "command:asof", kind: "command", label: "/asof", name: "/asof",
      detail: t("chatComposer.asofDetail"), group: t("chatComposer.commands"),
    };
    if (canSetAsOf && rankText(query, ["asof", asof.detail])) {
      if (query && "asof".startsWith(query.toLowerCase())) options.unshift(asof);
      else options.push(asof);
    }
    return options;
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the interface language is the recompute trigger; `i18n` itself is not reactive.
  }, [activeTrigger, contextOptions, availableSkills, canSetAsOf, i18n.language]);

  useEffect(() => {
    if (activeTrigger?.symbol !== "@" || !canSearchContext) {
      setContextOptions([]);
      return;
    }
    let cancelled = false;
    const query = activeTrigger.query;
    const handle = window.setTimeout(() => {
      void searchContextRef.current!(query).then((options) => {
        if (!cancelled) setContextOptions(options);
      }).catch(() => {
        if (!cancelled) setContextOptions([]);
      });
    }, 120);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- a language change re-runs the context search so its results are worded in that language.
  }, [activeTrigger?.query, activeTrigger?.symbol, canSearchContext, i18n.language]);

  useEffect(() => {
    setActiveOption(0);
  }, [activeTrigger?.query, activeTrigger?.symbol]);

  useEffect(() => {
    if (!prompt) setActiveTrigger(null);
  }, [prompt]);

  useEffect(() => {
    assistRef.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView?.({ block: "nearest" });
  }, [activeOption]);

  const syncTrigger = (target: HTMLTextAreaElement) => {
    const insideReference = inline.references.some(reference => reference.start < target.selectionStart && target.selectionStart <= reference.end);
    setActiveTrigger(insideReference ? null : activeTriggerAt(target.value, target.selectionStart));
  };

  const syncHighlightScroll = (target: HTMLTextAreaElement) => {
    const highlight = highlightRef.current;
    if (!highlight) return;
    // A classic (always shown) scrollbar narrows the textarea's lines; the highlight wraps at the same width.
    const style = getComputedStyle(target);
    const scrollbar = Math.max(0, target.offsetWidth - target.clientWidth - (parseFloat(style.borderLeftWidth) || 0) - (parseFloat(style.borderRightWidth) || 0));
    highlight.style.setProperty("--composer-scrollbar", `${scrollbar}px`);
    highlight.scrollTop = target.scrollTop;
    highlight.scrollLeft = target.scrollLeft;
  };

  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    fitPromptHeight(textarea);
    syncHighlightScroll(textarea);
  }, [prompt]);

  // A wider or narrower window rewraps the text, so the message is fitted again.
  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea || typeof ResizeObserver === "undefined") return;
    let width = textarea.clientWidth;
    const observer = new ResizeObserver(() => {
      if (textarea.clientWidth === width) return;
      fitPromptHeight(textarea);
      syncHighlightScroll(textarea);
      width = textarea.clientWidth;
    });
    observer.observe(textarea);
    return () => observer.disconnect();
  }, []);

  const clearTrigger = () => {
    if (!activeTrigger) return;
    const next = `${prompt.slice(0, activeTrigger.start)}${prompt.slice(activeTrigger.end)}`.replace(/[ \t]{2,}/gu, " ");
    inline.edit(next);
    setActiveTrigger(null);
    requestAnimationFrame(() => textareaRef.current?.focus());
  };

  const addChip = (chip: ComposerChip) => {
    updateChips([...ownedChips.filter((existing) => existing.key !== chip.key), chip]);
  };

  const selectAssist = (option: AssistOption) => {
    if (!activeTrigger) return;
    const insertReference = (item: InlineComposerItem) => {
      inline.insert(item, activeTrigger.start, activeTrigger.end);
      setActiveTrigger(null);
    };
    if (option.kind === "command") {
      setDatePanel({ trigger: activeTrigger, initial: asOf });
      return;
    }
    if (option.context?.pin) { onAddKnowledgePin?.(option.context.pin); insertReference(pinItem(option.context.pin)); return; }
    if (option.context?.artifact) { onSelectArtifact?.(option.context.artifact); insertReference(artifactItem(option.context.artifact)); return; }
    if (option.context?.path || option.context?.connectionId) {
      const chip: ComposerChip = { key: option.id, kind: option.context?.path ? "file" : "connection", id: option.context?.path ?? option.context?.connectionId ?? option.id.replace(/^plugin:/u, ""), label: option.name };
      addChip(chip); insertReference(chipItem(chip)); return;
    }
    if (option.kind === "skill") addChip({ key: option.id, kind: "skill", id: option.id.replace(/^skill:/u, ""), label: option.name });
    clearTrigger();
  };

  /** `/asof 2026-03-03` before the cursor becomes the as-of chip; a date out of range opens the panel. */
  const applyTypedAsOf = (value: string, cursor: number) => {
    if (inline.references.some(reference => reference.start < cursor && cursor <= reference.end)) return false;
    const typed = onAsOfChange ? typedAsOf(value.slice(0, cursor)) : null;
    if (!typed) return false;
    const rest = `${value.slice(0, typed.start)}${value.slice(cursor)}`.replace(/[ \t]{2,}/gu, " ");
    inline.edit(rest.trimStart() === rest ? rest : rest.trimStart());
    setActiveTrigger(null);
    if (asOfProblem(typed.date)) setDatePanel({ trigger: null, initial: typed.date });
    else onAsOfChange?.(typed.date);
    return true;
  };

  const handlePromptChange = (event: ChangeEvent<HTMLTextAreaElement>) => {
    const { value, selectionStart } = event.target;
    inline.rememberSelection(event.target);
    if (!isComposing && value.slice(0, selectionStart).endsWith(" ") && applyTypedAsOf(value, selectionStart)) return;
    inline.edit(value);
    if (!isComposing) syncTrigger(event.target);
  };

  const closeDatePanel = (restoreMenu: boolean) => {
    const trigger = datePanel?.trigger ?? null;
    setDatePanel(null);
    if (restoreMenu && trigger) setActiveTrigger(trigger);
    else if (trigger) {
      // Leaving the panel drops the typed `/as` that opened it.
      const next = `${prompt.slice(0, trigger.start)}${prompt.slice(trigger.end)}`.replace(/[ \t]{2,}/gu, " ");
      inline.edit(next);
    }
    requestAnimationFrame(() => textareaRef.current?.focus());
  };

  const keepComposerFocus = () => {
    textareaRef.current?.focus();
  };

  const preserveComposerFocus = (
    event: { preventDefault: () => void },
  ) => {
    event.preventDefault();
  };

  const queueFollowUp = () => {
    textareaRef.current?.form?.requestSubmit();
    keepComposerFocus();
  };

  const showAdmitted = (text: string) => {
    setGuidance({ text, phase: "admitted" });
    if (admittedTimer.current !== null) window.clearTimeout(admittedTimer.current);
    admittedTimer.current = window.setTimeout(() => {
      admittedTimer.current = null;
      setGuidance((current) => current?.phase === "admitted" && current.text === text ? null : current);
    }, 1600);
  };

  const steerCurrent = () => {
    // A second press while the first is still waiting must not drop the draft or start another steer.
    // The keyboard shortcut reaches here without the Guide button's disabled state.
    if (steering.current || sendDisabledReason) return;
    const source = prompt;
    const text = composeSubmission(source, ownedChips, inline.references);
    if (!text) return;
    if (hasAttachedContext) {
      setAttachmentError(t("chatComposer.useSendNextToIncludeAttachedFilesOrReferences"));
      return;
    }
    if (admittedTimer.current !== null) {
      window.clearTimeout(admittedTimer.current);
      admittedTimer.current = null;
    }
    steering.current = true;
    setGuidance({ text, phase: "pending" });
    const finish = (sent: boolean | void) => {
      steering.current = false;
      if (sent === false) {
        setGuidance((current) => current?.phase === "pending" && current.text === text ? null : current);
        return;
      }
      updateChips([]);
      showAdmitted(text);
    };
    void Promise.resolve(onSend(text, [], source)).then(finish, () => finish(false));
    keepComposerFocus();
  };

  const handlePromptKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) {
      if (event.key === "Enter") event.preventDefault();
      return;
    }
    if (inline.keyDown(event)) return;
    if (event.key === "Enter" && !event.shiftKey && applyTypedAsOf(event.currentTarget.value, event.currentTarget.selectionStart)) {
      event.preventDefault();
      return;
    }
    if (event.key === "Backspace" && !event.currentTarget.value && event.currentTarget.selectionStart === 0) {
      // Backspace at the start of an empty message removes the last chip; the as-of chip goes last.
      if (ownedChips.length) { event.preventDefault(); updateChips(ownedChips.slice(0, -1)); return; }
      const lastPin = knowledgePins.at(-1);
      if (lastPin && onRemoveKnowledgePin) { event.preventDefault(); onRemoveKnowledgePin(lastPin); return; }
      if (selectedArtifact && onClearSelectedArtifact) { event.preventDefault(); onClearSelectedArtifact(); return; }
      if (asOf && onAsOfChange) { event.preventDefault(); onAsOfChange(undefined); return; }
    }
    if (activeTrigger) {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setActiveTrigger(null);
        return;
      }
      if (assistOptions.length > 0) {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          setActiveOption((current) => (current + (event.key === "ArrowDown" ? 1 : -1) + assistOptions.length) % assistOptions.length);
          return;
        }
        if ((event.key === "Enter" || event.key === "Tab") && !event.shiftKey && !event.metaKey && !event.ctrlKey) {
          event.preventDefault();
          selectAssist(assistOptions[activeOption] ?? assistOptions[0]);
          return;
        }
      }
    }
    // Esc stops the reply in progress (the Stop tile's shortcut); an open menu took it above, and a
    // layer over the conversation closes first.
    if (isRunning && event.key === "Escape" && !isStopping && !layerOpen) {
      event.preventDefault();
      onAbort();
      return;
    }
    // Enter queues. ⌘Enter / Ctrl+Enter guides the reply in progress. Shift+Enter stays a new line.
    if (isRunning && event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      if (!isSaving && (event.metaKey || event.ctrlKey)) steerCurrent();
      else queueFollowUp();
    }
  };

  const submission = composeSubmission(prompt, ownedChips, inline.references);
  const showSend = isRunning && !sendDisabledReason && (Boolean(submission) || attachmentCount > 0);
  const showGuide = isRunning && !isSaving && Boolean(submission);
  const canSteer = showGuide && !sendDisabledReason && !hasAttachedContext;
  const showRunHelp = isRunning && (Boolean(submission) || hasAttachedContext);

  return (
    // Not aria-busy while a reply runs: the reply is what changes, and a busy composer would hold back
    // its own status region (start, stop, guidance) until the reply ended.
    <div className="composer-shell"
      onChangeCapture={(event) => {
        if (event.target instanceof HTMLInputElement && event.target.type === "file") setAttachmentError(null);
      }}
      onPasteCapture={(event) => {
        const items = event.clipboardData?.items;
        if (items && [...items].some((item) => item.kind === "file")) setAttachmentError(null);
      }}
      onDropCapture={(event) => {
        if (event.dataTransfer.types.includes("Files")) setAttachmentError(null);
      }}>
      <span className="sr-only" role="status" aria-live="polite">
        {streamAnnouncement}
      </span>

      {(error || attachmentError) && (
        <Alert variant="destructive" className="composer-error" role="alert">
          <AlertDescription>{error ?? attachmentError}</AlertDescription>
        </Alert>
      )}

      {datePanel ? (
        <AsOfDatePanel
          initial={datePanel.initial}
          onSet={(date) => { onAsOfChange?.(date); closeDatePanel(false); }}
          onCancel={() => closeDatePanel(false)}
          onBack={() => closeDatePanel(true)}
        />
      ) : activeTrigger ? (
        <ComposerAssistMenu
          id={assistId}
          menuRef={assistRef}
          trigger={activeTrigger}
          options={assistOptions}
          activeOption={activeOption}
          onHover={setActiveOption}
          onSelect={selectAssist}
        />
      ) : null}

      <PromptInput
        attachmentDraft={attachmentDraft}
        className={cn(
          "composer",
          isRunning && "composer-running",
          privateChat && "composer-private",
          isStopping && "composer-stopping",
        )}
        multiple
        maxFiles={4}
        maxFileSize={25 * 1024 * 1024}
        validateFile={validateAttachment}
        onError={({ message }) => setAttachmentError(message)}
        onSubmit={async ({ text, files }) => {
          if (sendDisabledReason) return false;
          const next = composeSubmission(text, ownedChips, inline.references);
          if (!next && files.length === 0) return false;
          if (
            files.some((file) => file.mediaType.startsWith("image/")) &&
            !selectedModel?.input.includes("image")
          ) {
            setAttachmentError(t("chatComposer.theSelectedModelDoesNotAcceptImages"));
            return false;
          }
          setAttachmentError(null);
          if (isRunning) {
            const queued = await onFollowUp(next, files, text);
            keepComposerFocus();
            if (queued !== false) updateChips([]);
            return queued !== false;
          }
          const sent = (await onSend(next, files, text)) !== false;
          if (sent) {
            updateChips([]);
            // Sent from the tile with the keyboard: the emptied message disables it, so the message takes focus back.
            const focused = document.activeElement;
            if (!focused || focused === document.body || (focused instanceof HTMLElement && focused.closest(".composer-run-actions"))) keepComposerFocus();
          }
          return sent;
        }}
      >
        <ComposerAttachmentTray onFilesChange={onFilesChange} />
        <ComposerIncomingFiles request={incomingFiles} onTaken={onIncomingFilesTaken} />
        <ComposerDropOverlay />
        {/* Waiting is said here. Once accepted, the Guiding row the conversation log adds says it,
            so the accepted note is a new element that only shows. */}
        {guidance ? (
          <p key={guidance.phase} className="composer-guidance" data-state={guidance.phase} role={guidance.phase === "pending" ? "status" : undefined} title={guidance.text}>
            {t(guidance.phase === "pending" ? "chatComposer.guidancePending" : "chatComposer.guidanceAdmitted", { text: guidance.text })}
          </p>
        ) : null}
        {/* One row, in reading and tab order: +, the message, the model label, then Send or Stop. */}
        <PromptInputFooter className="composer-footer">
          <ComposerAddMenu
            privateChat={privateChat}
            onPrivateChatChange={onPrivateChatChange}
            onAttachmentError={setAttachmentError}
            mapScreenshot={mapScreenshot}
            readClipboardImage={readClipboardImage}
            evidenceSources={evidenceSources}
            onAddKnowledgePin={onAddKnowledgePin}
          />
        <PromptInputBody>
          <div
            className={cn("composer-prompt", isComposing && "is-composing")}
            onCompositionStart={() => setIsComposing(true)}
            onCompositionEnd={() => setIsComposing(false)}
          >
            <ComposerPromptHighlight
              prompt={prompt}
              skills={availableSkills}
              plugins={plugins}
              highlightRef={highlightRef}
              references={inline.references}
            />
            <PromptInputTextarea
              ref={textareaRef}
              data-chat-composer="true" aria-label={inputLabel} rows={1}
              aria-describedby={showRunHelp ? runHelpId : undefined}
              aria-keyshortcuts={isRunning ? "Enter Shift+Enter Control+Enter Meta+Enter" : "Enter Shift+Enter"}
              aria-autocomplete="list"
              aria-controls={activeTrigger ? assistId : undefined}
              aria-activedescendant={activeTrigger && assistOptions.length ? `${assistId}-${activeOption}` : undefined}
              placeholder={asOf ? t("asOf.placeholder", { date: formatAsOf(asOf) }) : !accountConnected && onOpenModelSettings ? t("chatComposer.connectToAsk") : placeholder}
              value={prompt}
              onChange={handlePromptChange}
              onSelect={event => inline.rememberSelection(event.currentTarget)}
              onBlur={event => { inline.rememberSelection(event.currentTarget); setActiveTrigger(null); }}
              onClick={(event) => { inline.rememberSelection(event.currentTarget); syncTrigger(event.currentTarget); }}
              onScroll={(event) => syncHighlightScroll(event.currentTarget)}
              onKeyDown={handlePromptKeyDown}
              onKeyUp={(event) => {
                if (
                  ["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)
                ) {
                  syncTrigger(event.currentTarget);
                }
              }}
            />
          </div>
        </PromptInputBody>
          <PromptInputTools className="composer-tools">
          {/* Without an AI account the connect action takes the send slot. */}
          {accountConnected ? <ChatModelMenu
            models={models}
            providers={providers}
            selection={modelSelection}
            openSignal={openModelMenuSignal}
            onSelectionChange={onModelSelectionChange}
            onManageModels={onOpenModelSettings}
            onOpenUsage={onOpenUsage}
            modelName={modelName}
            isDefault={modelIsDefault}
            onChooseDefault={onChooseDefault}
          /> : null}
          {!isRunning && !accountConnected && onOpenModelSettings ? (
            <ChatModelMenu noAccount models={models} providers={providers} selection={modelSelection} onSelectionChange={onModelSelectionChange}
              onManageModels={onOpenModelSettings} isDefault={modelIsDefault} onChooseDefault={onChooseDefault} />
          ) : (
            <div className="composer-run-actions">
              {isRunning ? <ComposerRunExtras
                showSend={showSend}
                showGuide={showGuide}
                canSteer={canSteer}
                guidePending={guidance?.phase === "pending"}
                onQueue={queueFollowUp}
                onSteer={steerCurrent}
                onPreserveFocus={preserveComposerFocus}
              /> : null}
              <ComposerTile
                running={isRunning}
                saving={isSaving}
                stopping={isStopping}
                prompt={submission}
                hasModel={Boolean(
                  !sendDisabledReason && selectedModel && selectedModel.credential_source !== "missing" &&
                  modelSelection.thinking_level,
                )}
                disabledReason={
                  sendDisabledReason ?? (selectedModel?.credential_source === "missing"
                    ? t("chatComposer.connectAModelProvider")
                    : modelSelection.model_profile_id
                    ? t("chatComposer.chooseAThinkingLevel")
                    : t("chatComposer.chooseAModel"))
                }
                onAbort={onAbort}
                onPreserveFocus={preserveComposerFocus}
              />
            </div>
          )}
          </PromptInputTools>
        </PromptInputFooter>
        {/* What applies to this message (private chat, Skill modes, the as-of date) sits under the row.
            Private chat says in visible text, under its chip, what it keeps and what still leaves the Mac. */}
        {privateChat ? <div className="composer-private-status"><span className="composer-private-chip">
          <EyeOffIcon aria-hidden="true" /><span>{t("chatComposer.privateChip")}</span>
          <button type="button" aria-label={t("chatComposer.privateOff")} onClick={() => { onPrivateChatChange(false); keepComposerFocus(); }}><XIcon aria-hidden="true" /></button>
        </span><p className="composer-private-note">{t("chatComposer.privateStatus")}</p></div> : null}
        <ComposerChipTray
          chips={ownedChips.filter(chip => chip.kind === "skill")}
          asOf={asOf}
          onRemoveChip={(chip) => { updateChips(ownedChips.filter((existing) => existing.key !== chip.key)); keepComposerFocus(); }}
          onEditAsOf={() => setDatePanel({ trigger: null, initial: asOf })}
          onRemoveAsOf={() => { onAsOfChange?.(undefined); textareaRef.current?.focus(); }}
        />
        <ComposerInlineReferenceControls items={inlineItems} onRemove={(key) => { inline.remove(key); keepComposerFocus(); }} />
        {showRunHelp ? <p id={runHelpId} className="composer-run-help">{isSaving
          ? t("memorySaving.queueHelp", { send: shortcutFor("send"), newline: shortcutFor("newLine") })
          : hasAttachedContext
          ? t("chatComposer.useSendNextToIncludeAttachedFilesOrReferences")
          : t("chatComposer.runHelp", { send: shortcutFor("send"), guide: shortcutFor("guide"), newline: shortcutFor("newLine") })}</p> : null}
      </PromptInput>
      <p className="composer-disclosure">{t("chatComposer.informationDisclosure")}</p>
    </div>
  );
}
