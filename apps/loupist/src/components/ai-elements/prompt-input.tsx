import i18n, { useTranslation } from "@/i18n";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupTextarea,
} from "@/components/ui/input-group";
import { Spinner } from "@/components/ui/spinner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { ChatStatus, FileUIPart } from "ai";
import {
  CornerDownLeftIcon,
  FileIcon,
  PlusIcon,
  SquareIcon,
  XIcon,
} from "lucide-react";
import type {
  ChangeEventHandler,
  ClipboardEventHandler,
  ComponentProps,
  FormEvent,
  FormEventHandler,
  HTMLAttributes,
  KeyboardEventHandler,
  ReactNode,
} from "react";
import {
  Children,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

const t = i18n.getFixedT(null, "chat");

// ============================================================================
// Helpers
// ============================================================================

const readFileDataUrl = (blob: Blob): Promise<string | null> => {
    // FileReader uses callback-based API, wrapping in Promise is necessary
    // oxlint-disable-next-line eslint-plugin-promise(avoid-new)
    return new Promise((resolve) => {
      const reader = new FileReader();
      // oxlint-disable-next-line eslint-plugin-unicorn(prefer-add-event-listener)
      reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : null);
      // oxlint-disable-next-line eslint-plugin-unicorn(prefer-add-event-listener)
      reader.onerror = () => resolve(null);
      reader.onabort = () => resolve(null);
      reader.readAsDataURL(blob);
    });
};

const convertBlobUrlToDataUrl = async (url: string): Promise<string | null> => {
  try {
    const response = await fetch(url);
    if (!response.ok) return null;
    const blob = await response.blob();
    return await readFileDataUrl(blob);
  } catch {
    return null;
  }
};

/** A file in the composer tray, with the size of the file the user picked. */
export type AttachedFile = FileUIPart & { id: string; sizeBytes?: number };

const originalFiles = new WeakMap<AttachedFile, File>();
export function attachmentDataUrl(file: AttachedFile): Promise<string | null> {
  const original = originalFiles.get(file);
  if (original) return readFileDataUrl(original);
  return file.url.startsWith("blob:") ? convertBlobUrlToDataUrl(file.url) : Promise.resolve(file.url);
}

export interface AttachmentsContext {
  isSubmitting: boolean;
  files: AttachedFile[];
  add: (files: File[] | FileList) => void;
  remove: (id: string) => void;
  openFileDialog: () => void;
}

// ============================================================================
// Component Context & Hooks
// ============================================================================

const LocalAttachmentsContext = createContext<AttachmentsContext | null>(null);

export const usePromptInputAttachments = () => {
  useTranslation("chat");
  const context = useContext(LocalAttachmentsContext);
  if (!context) {
    throw new Error("usePromptInputAttachments must be used within PromptInput");
  }
  return context;
};

export type PromptInputActionAddAttachmentsProps = ComponentProps<
  typeof DropdownMenuItem
> & {
  label?: string;
};

export const PromptInputActionAddAttachments = ({
  label = t("promptInput.addPhotosOrFiles"),
  ...props
}: PromptInputActionAddAttachmentsProps) => {
  useTranslation("chat");
  const attachments = usePromptInputAttachments();

  const handleSelect = useCallback(
    (e: Event) => {
      e.preventDefault();
      attachments.openFileDialog();
    },
    [attachments]
  );

  return (
    <DropdownMenuItem {...props} onSelect={handleSelect}>
      {/* A file glyph, aligned like the menu's other rows: images are only one kind of attachment. */}
      <FileIcon /> {label}
    </DropdownMenuItem>
  );
};

export interface PromptInputMessage {
  text: string;
  files: FileUIPart[];
}

export type PromptAttachmentDraft = { files: AttachedFile[]; onFilesChanged?: (files: AttachedFile[]) => void };

export type PromptInputProps = Omit<
  HTMLAttributes<HTMLFormElement>,
  "onSubmit" | "onError"
> & {
  // e.g., "image/*" or leave undefined for any
  attachmentDraft?: PromptAttachmentDraft;
  accept?: string;
  multiple?: boolean;
  // Minimal constraints
  maxFiles?: number;
  // bytes
  maxFileSize?: number;
  validateFile?: (file: File) => string | undefined;
  onError?: (err: {
    code: "max_files" | "max_file_size" | "accept" | "validation" | "submit";
    message: string;
  }) => void;
  onSubmit: (
    message: PromptInputMessage,
    event: FormEvent<HTMLFormElement>
  ) => boolean | void | Promise<boolean | void>;
};

export const PromptInput = ({
  attachmentDraft,
  className,
  accept,
  multiple,
  maxFiles,
  maxFileSize,
  validateFile,
  onError,
  onSubmit,
  children,
  ...props
}: PromptInputProps) => {
  useTranslation("chat");
  const inputRef = useRef<HTMLInputElement | null>(null);
  const formRef = useRef<HTMLFormElement | null>(null);
  const [files, setFiles] = useState<AttachedFile[]>(() => attachmentDraft?.files ?? []);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const submittingRef = useRef(false);
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  const filesRef = useRef(files);
  useEffect(() => {
    if (!attachmentDraft || attachmentDraft.files === filesRef.current) return;
    filesRef.current = attachmentDraft.files;
    setFiles(attachmentDraft.files);
  }, [attachmentDraft, attachmentDraft?.files]);
  const updateFiles = useCallback((next: AttachedFile[]) => {
    filesRef.current = next;
    if (attachmentDraft) { attachmentDraft.files = next; attachmentDraft.onFilesChanged?.(next); }
    setFiles(next);
  }, [attachmentDraft]);

  const openFileDialog = useCallback(() => {
    inputRef.current?.click();
  }, []);

  const matchesAccept = useCallback(
    (f: File) => {
      if (!accept || accept.trim() === "") {
        return true;
      }

      const patterns = accept
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);

      return patterns.some((pattern) => {
        if (pattern.endsWith("/*")) {
          // e.g: image/* -> image/
          const prefix = pattern.slice(0, -1);
          return f.type.startsWith(prefix);
        }
        return f.type === pattern;
      });
    },
    [accept]
  );

  const add = useCallback(
    (fileList: File[] | FileList) => {
      const incoming = [...fileList];
      const accepted = incoming.filter((f) => matchesAccept(f));
      if (incoming.length && accepted.length === 0) {
        onError?.({
          code: "accept",
          message: t("promptInput.noFilesMatchTheAcceptedTypes"),
        });
        return;
      }
      let validationError: string | undefined;
      const validated = accepted.filter((file) => {
        const message = validateFile?.(file);
        if (message) validationError ??= message;
        return !message;
      });
      if (validationError) onError?.({ code: "validation", message: validationError });
      if (!validated.length) return;
      const withinSize = (f: File) =>
        maxFileSize ? f.size <= maxFileSize : true;
      const sized = validated.filter(withinSize);
      if (sized.length < validated.length) {
        onError?.({
          code: "max_file_size",
          message: t("promptInput.fileSizeLimit", { value1: Math.round((maxFileSize ?? 0) / (1024 * 1024)) }),
        });
      }
      if (!sized.length) return;

      const previous = filesRef.current;
      const capacity = maxFiles === undefined ? sized.length : Math.max(0, maxFiles - previous.length);
      const capped = sized.slice(0, capacity);
      if (sized.length > capacity) {
        onError?.({ code: "max_files", message: t("promptInput.tooManyFilesSomeWereNotAdded") });
      }
      // Blob URLs belong to an attachment event, never a replayable React updater.
      const next: AttachedFile[] = capped.map((file) => ({
        filename: file.name,
        sizeBytes: file.size,
        id: globalThis.crypto.randomUUID(),
        mediaType: file.type,
        type: "file",
        url: URL.createObjectURL(file),
      }));
      next.forEach((attachment, i) => originalFiles.set(attachment, capped[i]));
      if (next.length) updateFiles([...previous, ...next]);
    },
    [matchesAccept, maxFiles, maxFileSize, onError, updateFiles, validateFile]
  );

  const remove = useCallback(
    (id: string) => {
      const previous = attachmentDraft?.files ?? filesRef.current;
      const found = previous.find((file) => file.id === id);
      if (!found) return;
      URL.revokeObjectURL(found.url);
      updateFiles(previous.filter((file) => file.id !== id));
    },
    [attachmentDraft, updateFiles]
  );

  useEffect(() => {
    const form = formRef.current;
    if (!form) {
      return;
    }

    const onDragOver = (e: DragEvent) => {
      if (e.dataTransfer?.types?.includes("Files")) {
        e.preventDefault();
      }
    };
    const onDrop = (e: DragEvent) => {
      if (e.dataTransfer?.types?.includes("Files")) {
        e.preventDefault();
      }
      if (e.dataTransfer?.files && e.dataTransfer.files.length > 0) {
        add(e.dataTransfer.files);
      }
    };
    form.addEventListener("dragover", onDragOver);
    form.addEventListener("drop", onDrop);
    return () => {
      form.removeEventListener("dragover", onDragOver);
      form.removeEventListener("drop", onDrop);
    };
  }, [add]);

  useEffect(
    () => () => {
      if (attachmentDraft) return;
      for (const file of filesRef.current) {
        if (file.url) {
          URL.revokeObjectURL(file.url);
        }
      }
    },
    []
  );

  const handleChange: ChangeEventHandler<HTMLInputElement> = useCallback(
    (event) => {
      if (event.currentTarget.files) {
        add(event.currentTarget.files);
      }
      // Reset input value to allow selecting files that were previously removed
      event.currentTarget.value = "";
    },
    [add]
  );

  const attachmentsCtx = useMemo<AttachmentsContext>(
    () => ({
      isSubmitting,
      add,
      files,
      openFileDialog,
      remove,
    }),
    [files, add, remove, openFileDialog, isSubmitting]
  );

  const handleSubmit: FormEventHandler<HTMLFormElement> = useCallback(
    async (event) => {
      event.preventDefault();
      if (submittingRef.current) return;
      submittingRef.current = true;
      setIsSubmitting(true);

      const form = event.currentTarget;
      const formData = new FormData(form);
      const text = (formData.get("message") as string) || "";

      try {
        // Convert blob URLs to data URLs asynchronously
        const convertedFiles: FileUIPart[] = await Promise.all(
          files.map(async ({ id: _id, ...item }) => {
            if (item.url?.startsWith("blob:")) {
              const dataUrl = await convertBlobUrlToDataUrl(item.url);
              if (!dataUrl) throw new Error(t("promptInput.couldNotReadAnAttachedFileRemoveItAnd"));
              return {
                ...item,
                url: dataUrl,
              };
            }
            return item;
          })
        );

        if (!mountedRef.current) return;
        const accepted = await onSubmit({ files: convertedFiles, text }, event);
        if (accepted !== false) {
          // Preserve any files added while this snapshot was being submitted.
          files.forEach((file) => remove(file.id));
          if (new FormData(form).get("message") === text) form.reset();
        }
      } catch {
        if (mountedRef.current) onError?.({
          code: "submit",
          message: t("promptInput.couldNotReadOrSendTheAttachedMessageYour"),
        });
      } finally {
        submittingRef.current = false;
        if (mountedRef.current) setIsSubmitting(false);
      }
    },
    [files, onSubmit, onError, remove]
  );

  return (
    <LocalAttachmentsContext.Provider value={attachmentsCtx}>
      <input
        accept={accept}
        aria-label={t("promptInput.uploadFiles")}
        className="hidden"
        multiple={multiple}
        onChange={handleChange}
        ref={inputRef}
        title={t("promptInput.uploadFiles")}
        type="file"
      />
      <form
        className={cn("w-full", className)}
        onSubmit={handleSubmit}
        ref={formRef}
        {...props}
      >
        <InputGroup className="overflow-hidden has-disabled:opacity-100">{children}</InputGroup>
      </form>
    </LocalAttachmentsContext.Provider>
  );
};

export type PromptInputBodyProps = HTMLAttributes<HTMLDivElement>;

export const PromptInputBody = ({
  className,
  ...props
}: PromptInputBodyProps) => (
  <div className={cn("contents", className)} {...props} />
);

export type PromptInputTextareaProps = ComponentProps<
  typeof InputGroupTextarea
>;

export const PromptInputTextarea = ({
  onChange,
  onKeyDown,
  readOnly,
  className,
  placeholder = t("promptInput.whatWouldYouLikeToKnow"),
  ...props
}: PromptInputTextareaProps) => {
  useTranslation("chat");
  const attachments = usePromptInputAttachments();
  const [isComposing, setIsComposing] = useState(false);

  const handleKeyDown: KeyboardEventHandler<HTMLTextAreaElement> = useCallback(
    (e) => {
      // Call the external onKeyDown handler first
      if (attachments.isSubmitting) {
        if (e.key === "Enter" && !e.shiftKey && !isComposing && !e.nativeEvent.isComposing) e.preventDefault();
        return;
      }
      onKeyDown?.(e);

      // If the external handler prevented default, don't run internal logic
      if (e.defaultPrevented) {
        return;
      }

      if (e.key === "Enter") {
        if (isComposing || e.nativeEvent.isComposing) {
          return;
        }
        if (e.shiftKey) {
          return;
        }
        e.preventDefault();

        // Check if the submit button is disabled before submitting
        const { form } = e.currentTarget;
        const submitButton = form?.querySelector(
          'button[type="submit"]'
        ) as HTMLButtonElement | null;
        if (submitButton?.disabled) {
          return;
        }

        form?.requestSubmit();
      }

      // Remove last attachment when Backspace is pressed and textarea is empty
      if (
        e.key === "Backspace" &&
        e.currentTarget.value === "" &&
        attachments.files.length > 0
      ) {
        e.preventDefault();
        const lastAttachment = attachments.files.at(-1);
        if (lastAttachment) {
          attachments.remove(lastAttachment.id);
        }
      }
    },
    [onKeyDown, isComposing, attachments]
  );

  const handlePaste: ClipboardEventHandler<HTMLTextAreaElement> = useCallback(
    (event) => {
      const items = event.clipboardData?.items;

      if (!items) {
        return;
      }

      const files: File[] = [];

      for (const item of items) {
        if (item.kind === "file") {
          const file = item.getAsFile();
          if (file) {
            files.push(file);
          }
        }
      }

      if (files.length > 0) {
        event.preventDefault();
        attachments.add(files);
      }
    },
    [attachments]
  );

  const handleCompositionEnd = useCallback(() => setIsComposing(false), []);
  const handleCompositionStart = useCallback(() => setIsComposing(true), []);

  return (
    <InputGroupTextarea
      className={cn("field-sizing-content max-h-48 min-h-16", className)}
      name="message"
      onCompositionEnd={handleCompositionEnd}
      onCompositionStart={handleCompositionStart}
      onChange={onChange}
      onKeyDown={handleKeyDown}
      onPaste={handlePaste}
      placeholder={placeholder}
      {...props}
      readOnly={readOnly}
    />
  );
};

export type PromptInputFooterProps = Omit<
  ComponentProps<typeof InputGroupAddon>,
  "align"
>;

export const PromptInputFooter = ({
  className,
  ...props
}: PromptInputFooterProps) => (
  <InputGroupAddon
    align="block-end"
    className={cn("justify-between gap-1", className)}
    {...props}
  />
);

export type PromptInputToolsProps = HTMLAttributes<HTMLDivElement>;

export const PromptInputTools = ({
  className,
  ...props
}: PromptInputToolsProps) => (
  <div
    className={cn("flex min-w-0 items-center gap-1", className)}
    {...props}
  />
);

export type PromptInputButtonTooltip =
  | string
  | {
      content: ReactNode;
      shortcut?: string;
      side?: ComponentProps<typeof TooltipContent>["side"];
    };

export type PromptInputButtonProps = ComponentProps<typeof InputGroupButton> & {
  tooltip?: PromptInputButtonTooltip;
};

export const PromptInputButton = ({
  variant = "ghost",
  className,
  size,
  tooltip,
  ...props
}: PromptInputButtonProps) => {
  useTranslation("chat");
  const newSize =
    size ?? (Children.count(props.children) > 1 ? "sm" : "icon-sm");

  const button = (
    <InputGroupButton
      className={cn(className)}
      size={newSize}
      type="button"
      variant={variant}
      {...props}
    />
  );

  if (!tooltip) {
    return button;
  }

  const tooltipContent =
    typeof tooltip === "string" ? tooltip : tooltip.content;
  const shortcut = typeof tooltip === "string" ? undefined : tooltip.shortcut;
  const side = typeof tooltip === "string" ? "top" : (tooltip.side ?? "top");

  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent side={side}>
        {tooltipContent}
        {shortcut && (
          <span className="ml-2 text-muted-foreground">{shortcut}</span>
        )}
      </TooltipContent>
    </Tooltip>
  );
};

export type PromptInputActionMenuProps = ComponentProps<typeof DropdownMenu>;
export const PromptInputActionMenu = (props: PromptInputActionMenuProps) => (
  <DropdownMenu {...props} />
);

export type PromptInputActionMenuTriggerProps = PromptInputButtonProps;

export const PromptInputActionMenuTrigger = ({
  className,
  children,
  ...props
}: PromptInputActionMenuTriggerProps) => (
  <DropdownMenuTrigger asChild>
    <PromptInputButton className={className} {...props}>
      {children ?? <PlusIcon className="size-4" />}
    </PromptInputButton>
  </DropdownMenuTrigger>
);

export type PromptInputActionMenuContentProps = ComponentProps<
  typeof DropdownMenuContent
>;
export const PromptInputActionMenuContent = ({
  className,
  ...props
}: PromptInputActionMenuContentProps) => (
  <DropdownMenuContent align="start" className={cn(className)} {...props} />
);

export type PromptInputActionMenuItemProps = ComponentProps<
  typeof DropdownMenuItem
>;
export const PromptInputActionMenuItem = ({
  className,
  ...props
}: PromptInputActionMenuItemProps) => (
  <DropdownMenuItem className={cn(className)} {...props} />
);

// Note: Actions that perform side-effects (like opening a file dialog)
// are provided in opt-in modules (e.g., prompt-input-attachments).

export type PromptInputSubmitProps = ComponentProps<typeof InputGroupButton> & {
  status?: ChatStatus;
  onStop?: () => void;
};

export const PromptInputSubmit = ({
  className,
  variant = "default",
  size = "icon-sm",
  status,
  onStop,
  onClick,
  children,
  ...props
}: PromptInputSubmitProps) => {
  useTranslation("chat");
  const isGenerating = status === "submitted" || status === "streaming";

  let Icon = <CornerDownLeftIcon className="size-4" />;

  if (status === "submitted") {
    Icon = <Spinner />;
  } else if (status === "streaming") {
    Icon = <SquareIcon className="size-4" />;
  } else if (status === "error") {
    Icon = <XIcon className="size-4" />;
  }

  const handleClick = useCallback(
    (e: React.MouseEvent<HTMLButtonElement>) => {
      if (isGenerating && onStop) {
        e.preventDefault();
        onStop();
        return;
      }
      onClick?.(e);
    },
    [isGenerating, onStop, onClick]
  );

  return (
    <InputGroupButton
      aria-label={isGenerating ? t("promptInput.stop") : t("promptInput.submit")}
      className={cn(className)}
      onClick={handleClick}
      size={size}
      type={isGenerating && onStop ? "button" : "submit"}
      variant={variant}
      {...props}
    >
      {children ?? Icon}
    </InputGroupButton>
  );
};
