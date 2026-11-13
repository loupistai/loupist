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
import { skillPresentation } from "@/lib/agentSkills";
import { formatNumber } from "@/i18n/format";
import { composeSubmission, composerChipInlineText, contextGroupLabel, rankText, type ComposerChip, type ContextOption } from "../../chat/composerChips";
import type { FileUIPart } from "ai";
import type { ChatArtifactRef, KnowledgePin } from "../../types";
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
import { activeTriggerAt, ComposerAssistMenu, ComposerPromptHighlight, type ActiveTrigger, type AssistOption, type ComposerOption } from "./ComposerAssist";
import { AsOfDatePanel } from "./ComposerAsOf";
import { shortcutFor } from "../../app/shortcuts";
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
  isStopping?: boolean;
  error: string | null;
  sendDisabledReason?: string;
  streamAnnouncement: string;
  models: ConfiguredModel[];
  providers: ModelProviderOption[];
  modelSelection: ModelRunSelection;
  skills: ComposerOption[];
  plugins: ComposerOption[];
  knowledgePins?: readonly KnowledgePin[];
  chips?: readonly ComposerChip[];
  selectedArtifact?: ChatArtifactRef;
  onClearSelectedArtifact?: () => void;
  onRemoveKnowledgePin?: (pin: KnowledgePin) => void;
  onChipsChange?: (chips: ComposerChip[]) => void;
  onSearchContext?: (query: string) => Promise<ContextOption[]>;
  onAddKnowledgePin?: (pin: KnowledgePin) => void;
  /** "Screenshot of the map" in the + menu; absent where no map is in reach. */
  mapScreenshot?: MapScreenshot;
  /** The chat's saved Sources, offered as "Add from Evidence". */
  evidenceSources?: readonly EvidenceSourceOption[];
  onSelectArtifact?: (artifact: ChatArtifactRef) => void;
  onNewChat?: () => void;
  onPromptChange: (prompt: string) => void;
  onPrivateChatChange: (enabled: boolean) => void;
  onModelSelectionChange: (selection: ModelRunSelection) => void;
  /** Nothing chosen for this Loupist: the composer shows "Auto". */
  modelAuto: boolean;
  onModelResetToAuto: () => void;
  onSend: (prompt: string, files: FileUIPart[], draftText?: string) => boolean | void | Promise<boolean | void>;
  onFollowUp: (text?: string, files?: FileUIPart[], draftText?: string) => boolean | void;
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
};

export type IncomingFiles = { id: number; files: File[] };

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

function ComposerAttachmentTray({ onFilesChange }: { onFilesChange: (count: number) => void}) {
  useTranslation("chat");
  const attachments = usePromptInputAttachments();
  useEffect(() => {
    onFilesChange(attachments.files.length);
  }, [attachments.files, onFilesChange]);
  if (attachments.files.length === 0) return null;
  const count = attachments.files.length;
  const name = (filename?: string) => filename ?? t("chatComposer.attachmentName");
  // The size of the file the user picked, in the one format files use everywhere ("1.2 MB").
  const size = (bytes?: number) => bytes === undefined ? "" : fileSize(bytes);
  const titled = (file: { filename?: string; sizeBytes?: number }) => [name(file.filename), size(file.sizeBytes)].filter(Boolean).join(" · ");
  return (
    <div className="composer-attachments" aria-label={t("chatComposer.attachments.count", { count, formattedCount: formatNumber(count) })}>
      {attachments.files.map((file) => file.mediaType.startsWith("image/") ? (
        <figure className="composer-thumb" key={file.id} title={titled(file)}>
          <img src={file.url} alt={name(file.filename)} />
          <button type="button" aria-label={t("chatComposer.removeAttachment", { value1: name(file.filename) })} onClick={() => attachments.remove(file.id)}>
            <XIcon aria-hidden="true" />
          </button>
        </figure>
      ) : (
        <span className="composer-file-chip" key={file.id} title={titled(file)}>
          <FileIcon aria-hidden="true" />
          <span className="composer-file-name">{name(file.filename)}</span>
          {file.sizeBytes === undefined ? null : <span className="composer-file-size gg-num">{size(file.sizeBytes)}</span>}
          <button type="button" aria-label={t("chatComposer.removeAttachment", { value1: name(file.filename) })} onClick={() => attachments.remove(file.id)}>
            <XIcon aria-hidden="true" />
          </button>
        </span>
      ))}
      <span className="composer-attachments-meta">
        {t("chatComposer.attachments.count", { count, formattedCount: formatNumber(count) })}
        {count >= 2 ? <button type="button" onClick={() => { for (const file of [...attachments.files]) attachments.remove(file.id); }}>{t("chatComposer.attachments.clearAll")}</button> : null}
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
}: {
  onError: (message: string | null) => void;
}) {
  useTranslation("chat");
  const attachments = usePromptInputAttachments();

  const attachClipboard = async () => {
    onError(null);
    if (!navigator.clipboard?.read) {
      onError(t("chatComposer.clipboardAttachmentsAreNotAvailableInThisEnvironment"));
      return;
    }
    try {
      const items = await navigator.clipboard.read();
      const files: File[] = [];
      for (const item of items) {
        for (const mediaType of item.types) {
          const blob = await item.getType(mediaType);
          if (!mediaType.startsWith("image/")) continue;
          const extension =
            mediaType === "image/png"
              ? "png"
              : mediaType === "image/jpeg"
                ? "jpg"
                : mediaType === "image/webp"
                  ? "webp"
                  : mediaType === "image/gif"
                    ? "gif"
                    : "img";
          files.push(
            new File([blob], `clipboard-${Date.now()}.${extension}`, {
              type: mediaType,
            }),
          );
        }
      }
      if (files.length === 0) {
        onError(t("chatComposer.theClipboardDoesNotContainAnAttachableImage"));
        return;
      }
      attachments.add(files);
    } catch (cause) {
      if (cause instanceof DOMException && cause.name === "NotAllowedError") {
        onError(t("chatComposer.clipboardAccessWasNotAllowed"));
      } else {
        onError(t("chatComposer.couldNotAttachClipboardContents"));
      }
    }
  };

  return (
    <PromptInputActionMenuItem
      onSelect={(event) => {
        event.preventDefault();
        void attachClipboard();
      }}
    >
      <ClipboardIcon />{t("chatComposer.attach.pasteImage")}</PromptInputActionMenuItem>
  );
}

function ComposerPromptTools({
  privateChat,
  models,
  providers,
  modelSelection,
  onPrivateChatChange,
  onModelSelectionChange,
  modelAuto,
  onModelResetToAuto,
  onAttachmentError,
  modelMenuSignal,
  onOpenModelSettings,
  onOpenUsage,
  mapScreenshot,
  evidenceSources,
  onAddKnowledgePin,
}: {
  privateChat: boolean;
  models: ConfiguredModel[];
  providers: ModelProviderOption[];
  modelSelection: ModelRunSelection;
  onPrivateChatChange: (enabled: boolean) => void;
  onModelSelectionChange: (selection: ModelRunSelection) => void;
  /** Nothing chosen for this Loupist: the composer shows "Auto". */
  modelAuto: boolean;
  onModelResetToAuto: () => void;
  onAttachmentError: (message: string | null) => void;
  modelMenuSignal: number;
  onOpenModelSettings?: () => void;
  onOpenUsage?: (provider: string) => void;
  mapScreenshot?: MapScreenshot;
  evidenceSources?: readonly EvidenceSourceOption[];
  onAddKnowledgePin?: (pin: KnowledgePin) => void;
}) {
  useTranslation("chat");
  const accountConnected = hasAccount(models, providers);
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
    <PromptInputTools className="composer-tools">
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
          <ClipboardAttachmentAction onError={onAttachmentError} />
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
      {/* Without an AI account the connect action takes the send slot (ChatComposer renders it there). */}
      {accountConnected ? <ChatModelMenu
        models={models}
        providers={providers}
        selection={modelSelection}
        openSignal={modelMenuSignal}
        onSelectionChange={onModelSelectionChange}
        onManageModels={onOpenModelSettings}
        onOpenUsage={onOpenUsage}
        auto={modelAuto}
        onResetToAuto={onModelResetToAuto}
      /> : null}
    </PromptInputTools>
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
 * Send and Stop are one 28 px tile at the composer's trailing edge (DIRECTION
 * 10.2). The same button stays mounted from idle to running and back: its two
 * icons cross-fade in place (120 ms) and its box never changes. Pressing it
 * leaves focus in the textarea. After Stop it dims and ignores presses until
 * the host confirms.
 */
function ComposerTile({
  running,
  prompt,
  hasModel,
  disabledReason,
  onAbort,
  onPreserveFocus,
}: {
  running: boolean;
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
  const empty = !prompt.trim() && attachments.files.length === 0;
  const label = running ? t("chatComposer.stopResponse") : t("chatComposer.send");
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
          title={!running && !hasModel ? disabledReason : undefined}
          onMouseDown={onPreserveFocus}
          onClick={running ? onAbort : undefined}
        >
          <span className="composer-tile-icon" data-icon="send" aria-hidden="true">{submitting ? <Spinner /> : <ArrowUpIcon />}</span>
          <span className="composer-tile-icon" data-icon="stop" aria-hidden="true"><SquareIcon /></span>
        </InputGroupButton>
      </TooltipTrigger>
      <TooltipContent side="top">
        {t("chatComposer.stopResponse")}
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
  evidenceSources,
  onSelectArtifact,
  onNewChat,
  onPromptChange,
  onPrivateChatChange,
  onModelSelectionChange,
  modelAuto,
  onModelResetToAuto,
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
}: Props) {
  useTranslation("chat");
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
  const [modelMenuSignal, setModelMenuSignal] = useState(0);
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
  const canSetAsOf = Boolean(onAsOfChange);
  const assistOptions = useMemo<AssistOption[]>(() => {
    if (!activeTrigger) return [];
    const query = activeTrigger.query;
    const commandQuery = query.trim().toLowerCase();
    const skillOptions = skills.flatMap((skill) => {
      // Bundled Skills show their localized name, as Settings › Skills does; the chip still invokes the id.
      const { name } = skillPresentation(skill);
      const score = rankText(query, [name, skill.name, skill.description, skill.id]);
      return score ? [{
        id: `skill:${skill.id}`,
        kind: "skill" as const,
        label: name,
        name,
        // Bundled Skills read a short line for people; the agent-facing description stays in the prompt metadata.
        detail: (t as unknown as (key: string, options: { defaultValue: string }) => string)(`skillBlurb.${skill.id}`, { defaultValue: skill.description }),
        group: t("chatComposer.skills"),
      }] : [];
    }).sort((left, right) => rankText(query, [right.name]) - rankText(query, [left.name]));
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
    // `/`: the finance commands, then Skills by name, then the composer's own actions.
    // Command names are typed literals, the same in every language.
    const finance = ([
      { id: "command:brief", kind: "command", label: "/brief", name: "/brief", detail: t("chatComposer.briefDetail"), group: t("chatComposer.commands"), command: "brief" },
      { id: "command:compare", kind: "command", label: "/compare", name: "/compare", detail: t("chatComposer.compareDetail"), group: t("chatComposer.commands"), command: "compare" },
      { id: "command:check", kind: "command", label: "/check", name: "/check", detail: t("chatComposer.checkDetail"), group: t("chatComposer.commands"), command: "check" },
      ...(canSetAsOf ? [{ id: "command:asof", kind: "command", label: "/asof", name: "/asof", detail: t("chatComposer.asofDetail"), group: t("chatComposer.commands"), command: "asof" }] as const : []),
    ] as const satisfies readonly AssistOption[]).filter((option) => !query.trim() || rankText(query, [option.name.slice(1), option.detail]) >= 2)
      // A command whose name starts with what was typed comes first: `/as` → /asof before /compare.
      .sort((a, b) => Number(b.name.slice(1).startsWith(commandQuery)) - Number(a.name.slice(1).startsWith(commandQuery)));
    const commands = ([
      { id: "command:new-chat", kind: "command", label: t("chatComposer.newChat"), name: t("chatComposer.newChat"), detail: t("chatComposer.startAFreshConversation"), group: t("chatComposer.commands"), command: "new-chat" },
      { id: "command:model", kind: "command", label: t("chatComposer.model"), name: t("chatComposer.model"), detail: t("chatComposer.chooseTheModel"), group: t("chatComposer.commands"), command: "model" },
      { id: "command:memory", kind: "command", label: t("chatComposer.memory"), name: t("chatComposer.memory"), detail: t("chatComposer.changeWhatThisMessageRemembers"), group: t("chatComposer.commands"), command: "memory" },
      { id: "command:attach", kind: "command", label: t("chatComposer.attachFiles"), name: t("chatComposer.attachFiles"), detail: t("chatComposer.addFilesOrImages"), group: t("chatComposer.commands"), command: "attach" },
    ] as const satisfies readonly AssistOption[]).filter((option) => {
      const score = rankText(query, [option.name, option.detail]);
      return query.trim() ? score >= 3 : score > 0;
    });
    const actions = commands.map((option) => ({ ...option, group: t("chatComposer.actions") }));
    return [...finance, ...skillOptions.slice(0, 8), ...actions];
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the interface language is the recompute trigger; `i18n` itself is not reactive.
  }, [activeTrigger, contextOptions, skills, canSetAsOf, i18n.language]);

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
    highlight.scrollTop = target.scrollTop;
    highlight.scrollLeft = target.scrollLeft;
  };

  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (textarea) syncHighlightScroll(textarea);
  }, [prompt]);

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

  const startCommand = (starter: string) => {
    if (!activeTrigger) return;
    const before = prompt.slice(0, activeTrigger.start);
    const after = prompt.slice(activeTrigger.end);
    const next = `${before}${starter}${after}`;
    const cursor = before.length + starter.length;
    inline.edit(next);
    setActiveTrigger(activeTriggerAt(next, cursor));
    requestAnimationFrame(() => {
      const textarea = textareaRef.current;
      if (!textarea) return;
      textarea.focus();
      textarea.setSelectionRange(cursor, cursor);
    });
  };

  const selectAssist = (option: AssistOption) => {
    if (!activeTrigger) return;
    const insertReference = (item: InlineComposerItem) => {
      inline.insert(item, activeTrigger.start, activeTrigger.end);
      setActiveTrigger(null);
    };
    if (option.command === "brief") return startCommand(t("chatComposer.briefPrompt"));
    if (option.command === "compare") return startCommand(t("chatComposer.comparePrompt"));
    if (option.command === "check") return startCommand(t("chatComposer.checkPrompt"));
    if (option.command === "asof") {
      setDatePanel({ trigger: activeTrigger, initial: asOf });
      return;
    }
    if (option.command) {
      if (option.command === "new-chat") onNewChat?.();
      if (option.command === "model") setModelMenuSignal((value) => value + 1);
      if (option.command === "memory") {
        onPrivateChatChange(!privateChat);
      }
      if (option.command === "attach") textareaRef.current?.form?.querySelector<HTMLInputElement>("input[type=file]")?.click();
      clearTrigger();
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
    // Enter queues. ⌘Enter / Ctrl+Enter guides the reply in progress. Shift+Enter stays a new line.
    if (isRunning && event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      if (event.metaKey || event.ctrlKey) steerCurrent();
      else queueFollowUp();
    }
  };

  const submission = composeSubmission(prompt, ownedChips, inline.references);
  const showSend = isRunning && !sendDisabledReason && (Boolean(submission) || attachmentCount > 0);
  const showGuide = isRunning && Boolean(submission);
  const canSteer = showGuide && !sendDisabledReason && !hasAttachedContext;
  const showRunHelp = isRunning && (Boolean(submission) || hasAttachedContext);

  return (
    <div className="composer-shell" aria-busy={isRunning}
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
            const queued = onFollowUp(next, files, text);
            keepComposerFocus();
            if (queued !== false) updateChips([]);
            return queued !== false;
          }
          const sent = (await onSend(next, files, text)) !== false;
          if (sent) updateChips([]);
          return sent;
        }}
      >
        <ComposerAttachmentTray onFilesChange={onFilesChange} />
        <ComposerIncomingFiles request={incomingFiles} onTaken={onIncomingFilesTaken} />
        <ComposerDropOverlay />
        <PromptInputBody>
          {guidance ? (
            <p className="composer-guidance" data-state={guidance.phase} role="status" title={guidance.text}>
              {t(guidance.phase === "pending" ? "chatComposer.guidancePending" : "chatComposer.guidanceAdmitted", { text: guidance.text })}
            </p>
          ) : null}
          <div
            className={cn("composer-prompt", isComposing && "is-composing")}
            onCompositionStart={() => setIsComposing(true)}
            onCompositionEnd={() => setIsComposing(false)}
          >
            <ComposerPromptHighlight
              prompt={prompt}
              skills={skills}
              plugins={plugins}
              highlightRef={highlightRef}
              references={inline.references}
            />
            <PromptInputTextarea
              ref={textareaRef}
              data-chat-composer="true" aria-label={inputLabel}
              aria-describedby={showRunHelp ? runHelpId : undefined}
              aria-keyshortcuts={isRunning ? "Enter Shift+Enter Control+Enter Meta+Enter" : "Enter Shift+Enter"}
              aria-autocomplete="list"
              aria-controls={activeTrigger ? assistId : undefined}
              aria-activedescendant={activeTrigger && assistOptions.length ? `${assistId}-${activeOption}` : undefined}
              placeholder={asOf ? t("asOf.placeholder", { date: formatAsOf(asOf) }) : !hasAccount(models, providers) && onOpenModelSettings ? t("chatComposer.connectToAsk") : placeholder}
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
        {privateChat ? <div className="composer-private-status"><span className="composer-private-chip" title={t("chatComposer.privateStatus")} aria-description={t("chatComposer.privateStatus")}>
          <EyeOffIcon aria-hidden="true" /><span>{t("chatComposer.privateChip")}</span>
          <button type="button" aria-label={t("chatComposer.privateOff")} onClick={() => onPrivateChatChange(false)}><XIcon aria-hidden="true" /></button>
        </span></div> : null}
        <ComposerChipTray
          chips={ownedChips.filter(chip => chip.kind === "skill")}
          asOf={asOf}
          onRemoveChip={(chip) => updateChips(ownedChips.filter((existing) => existing.key !== chip.key))}
          onEditAsOf={() => setDatePanel({ trigger: null, initial: asOf })}
          onRemoveAsOf={() => { onAsOfChange?.(undefined); textareaRef.current?.focus(); }}
        />
        <ComposerInlineReferenceControls items={inlineItems} onRemove={inline.remove} />
        <PromptInputFooter className="composer-footer">
          <ComposerPromptTools
            privateChat={privateChat}
            models={models}
            providers={providers}
            modelSelection={modelSelection}
            onPrivateChatChange={onPrivateChatChange}
            onModelSelectionChange={onModelSelectionChange}
            modelAuto={modelAuto}
            onModelResetToAuto={onModelResetToAuto}
            onAttachmentError={setAttachmentError}
            modelMenuSignal={modelMenuSignal + openModelMenuSignal}
            onOpenModelSettings={onOpenModelSettings}
            onOpenUsage={onOpenUsage}
            mapScreenshot={mapScreenshot}
            evidenceSources={evidenceSources}
            onAddKnowledgePin={onAddKnowledgePin}
          />
          {!isRunning && !hasAccount(models, providers) && onOpenModelSettings ? (
            <ChatModelMenu models={models} providers={providers} selection={modelSelection} onSelectionChange={onModelSelectionChange}
              onManageModels={onOpenModelSettings} auto={modelAuto} onResetToAuto={onModelResetToAuto} />
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
        </PromptInputFooter>
        {showRunHelp ? <p id={runHelpId} className="composer-run-help">{hasAttachedContext
          ? t("chatComposer.useSendNextToIncludeAttachedFilesOrReferences")
          : t("chatComposer.runHelp", { send: shortcutFor("send"), guide: shortcutFor("guide"), newline: shortcutFor("newLine") })}</p> : null}
      </PromptInput>
      <p className="composer-disclosure">{t("chatComposer.informationDisclosure")}</p>
    </div>
  );
}
