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
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { FileUIPart } from "ai";
import {
  FileIcon,
  PlusIcon,
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

/**
 * The file's bytes as a data: URL. The app's Content-Security-Policy allows `data:` images and no
 * `blob:` source, so an object URL could neither preview an image nor be fetched back on send
 * (docs/runtime/permissions.md#renderer-content-security-policy).
 */
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

const IMAGE_EXTENSIONS: Record<string, string> = { "image/png": "png", "image/jpeg": "jpg", "image/gif": "gif", "image/webp": "webp", "image/heic": "heic", "image/tiff": "tiff" };
const two = (value: number) => String(value).padStart(2, "0");

/**
 * A pasted picture under a name of its own ("Pasted image 2026-11-23 21.04.png"): WebKit names every
 * clipboard image "image.png", which would make several pasted images indistinguishable.
 */
export function pastedImage(file: Blob, at = new Date(), index = 0): File {
  const time = `${at.getFullYear()}-${two(at.getMonth() + 1)}-${two(at.getDate())} ${two(at.getHours())}.${two(at.getMinutes())}`;
  const name = `${t("promptInput.pastedImageName", { time })}${index ? ` (${index + 1})` : ""}.${IMAGE_EXTENSIONS[file.type] ?? "png"}`;
  return new File([file], name, { type: file.type || "image/png", lastModified: at.getTime() });
}

/** The clipboard's own name for an image it holds, rather than a file someone named. */
const unnamedClipboardImage = (file: File) => file.type.startsWith("image/") && (!file.name || /^image\.[a-z0-9]+$/iu.test(file.name));

/**
 * Text that only describes the files next to it: nothing, a file's own name (Finder puts it beside
 * the file) or an address (a browser's Copy Image). Any other text is what the user copied.
 */
const textOnlyNamesFiles = (text: string, files: readonly File[]) => {
  const value = text.trim();
  if (!value) return true;
  if (/^(?:https?|file):\/\/\S+$/iu.test(value)) return true;
  const names = new Set(files.map((file) => file.name));
  return value.split(/\r?\n/u).every((line) => names.has(line.trim()));
};

/**
 * The files a paste attaches, or none when it is a text paste. A copied spreadsheet range or web
 * selection carries a picture of itself next to its text; the text wins unless the clipboard holds
 * only images (or their names or address). A copied non-image file always attaches.
 */
export function pastedFiles(data: Pick<DataTransfer, "items" | "files"> & Partial<Pick<DataTransfer, "getData">>, at = new Date()): File[] {
  const fromItems = [...(data.items ?? [])].flatMap((item) => {
    if (item.kind !== "file") return [];
    const file = item.getAsFile();
    return file ? [file] : [];
  });
  const files = fromItems.length ? fromItems : [...(data.files ?? [])];
  if (!files.length) return [];
  const imagesOnly = files.every((file) => file.type.startsWith("image/"));
  if (imagesOnly && !textOnlyNamesFiles(data.getData?.("text/plain") ?? "", files)) return [];
  let unnamed = 0;
  return files.map((file) => unnamedClipboardImage(file) ? pastedImage(file, at, unnamed++) : file);
}

/** A file in the composer tray, with the size of the file the user picked. Its `url` is a data: URL of its bytes. */
export type AttachedFile = FileUIPart & { id: string; sizeBytes?: number };

export interface AttachmentsContext {
  isSubmitting: boolean;
  files: AttachedFile[];
  /** Files added and still being read; they join `files` when read. */
  reading: number;
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

  // The menu closes as the file dialog opens, so it is not left open behind the dialog or after the pick.
  const handleSelect = useCallback(() => {
    attachments.openFileDialog();
  }, [attachments]);

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
  // Reads in flight, each settling to whether all its files joined the tray; Send waits for them.
  const readsRef = useRef(new Set<Promise<boolean>>());
  const [reading, setReading] = useState(0);
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

      // Read the bytes once, when the file is added: the data: URL is the image's preview, the draft's
      // saved copy and what the host receives on send. The tray's count is checked when they are in.
      const pending = Promise.all(sized.map(readFileDataUrl)).then((urls) => {
        if (!mountedRef.current) return false;
        const read = sized.flatMap((file, i) => {
          const url = urls[i];
          return url?.startsWith("data:") ? [{ file, url }] : [];
        });
        if (read.length < sized.length) {
          onError?.({ code: "validation", message: t("promptInput.couldNotReadAnAttachedFileRemoveItAnd") });
        }
        const previous = filesRef.current;
        const capacity = maxFiles === undefined ? read.length : Math.max(0, maxFiles - previous.length);
        if (read.length > capacity) {
          onError?.({ code: "max_files", message: t("promptInput.tooManyFilesSomeWereNotAdded") });
        }
        const next: AttachedFile[] = read.slice(0, capacity).map(({ file, url }) => ({
          filename: file.name,
          sizeBytes: file.size,
          id: globalThis.crypto.randomUUID(),
          mediaType: file.type,
          type: "file",
          url,
        }));
        if (next.length) updateFiles([...previous, ...next]);
        return read.length === sized.length && next.length === read.length;
      }, () => false);
      readsRef.current.add(pending);
      setReading((count) => count + sized.length);
      void pending.then(() => {
        readsRef.current.delete(pending);
        if (mountedRef.current) setReading((count) => count - sized.length);
      });
    },
    [matchesAccept, maxFiles, maxFileSize, onError, updateFiles, validateFile]
  );

  const remove = useCallback(
    (id: string) => {
      const previous = attachmentDraft?.files ?? filesRef.current;
      const found = previous.find((file) => file.id === id);
      if (!found) return;
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
      reading,
      openFileDialog,
      remove,
    }),
    [files, add, remove, openFileDialog, isSubmitting, reading]
  );

  /** Waits for the files still being read; false when one of them did not join the tray. */
  const settleReads = useCallback(async () => {
    let complete = true;
    for (let pending = [...readsRef.current]; pending.length; pending = [...readsRef.current]) {
      const results = await Promise.all(pending);
      for (const read of pending) readsRef.current.delete(read);
      complete &&= results.every(Boolean);
    }
    return complete;
  }, []);

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
        // A file still being read goes with this message: Send waits for it. One that could not be read
        // or did not fit already said so, and the message stays for the user to send again.
        if (!(await settleReads())) return;
        const files = filesRef.current;
        // Each file was read into a data: URL when it was added (or restored as one with the draft).
        const submitted: FileUIPart[] = files.map(({ id: _id, ...item }) => {
          if (!item.url.startsWith("data:")) throw new Error(t("promptInput.couldNotReadAnAttachedFileRemoveItAnd"));
          return item;
        });
        const accepted = await onSubmit({ files: submitted, text }, event);
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
    [onSubmit, onError, remove, settleReads]
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

  // ⌘V of a screenshot, a copied image or a file copied in Finder attaches it, through the same
  // checks as the + menu and a drop (size, count, type). A text paste stays the textarea's own.
  const handlePaste: ClipboardEventHandler<HTMLTextAreaElement> = useCallback(
    (event) => {
      if (!event.clipboardData) return;
      const files = pastedFiles(event.clipboardData);
      if (!files.length) return;
      event.preventDefault();
      attachments.add(files);
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

  const buttonRef = useRef<HTMLButtonElement>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  useEffect(() => {
    const node = buttonRef.current;
    if (!node) return undefined;
    // A menu trigger sets data-state="open" on this button. The tooltip would otherwise stay up over the menu.
    const sync = () => setMenuOpen(node.dataset.state === "open");
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(node, { attributes: true, attributeFilter: ["data-state"] });
    return () => observer.disconnect();
  }, []);
  const button = (
    <InputGroupButton
      ref={buttonRef}
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
    <Tooltip open={menuOpen ? false : undefined}>
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

