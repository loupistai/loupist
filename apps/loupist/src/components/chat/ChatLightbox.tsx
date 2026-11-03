import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ChevronLeftIcon, ChevronRightIcon, DownloadIcon, PlusIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import type { ChatAttachment, ChatMessage } from "../../types";
import { fileSize } from "../../chat/fileSize";

const t = i18n.getFixedT(null, "chat");

/** An image the user sent in this chat, in the order the thread shows it. */
type ChatImage = { key: string; messageId: string; attachment: ChatAttachment; sentAt: string };

type ImageState = { url?: string; failed: boolean };

type ChatLightboxValue = {
  /** Starts reading an image once; thumbnails, the rail and the lightbox share the result. */
  load: (messageId: string, attachment: ChatAttachment) => void;
  image: (messageId: string, attachment: ChatAttachment) => ImageState;
  /** `from` is the control that opened it; closing returns focus there. */
  open: (messageId: string, attachmentId: string, from?: HTMLElement) => void;
};

const ChatLightboxContext = createContext<ChatLightboxValue | null>(null);

/** The chat's lightbox, or null outside a chat (a briefing reader, an isolated card). */
export const useChatLightbox = () => useContext(ChatLightboxContext);

const imageKey = (messageId: string, attachmentId: string) => `${messageId}:${attachmentId}`;
const isImage = (attachment: ChatAttachment) => attachment.media_type.startsWith("image/");

/**
 * The image as a file the composer can attach again (it keeps its name and
 * type). Rust hands images over as data URLs; a just-sent one may still be a
 * local blob URL.
 */
export async function attachmentFile(url: string, attachment: Pick<ChatAttachment, "filename" | "media_type">) {
  const data = /^data:([^,;]*)((?:;[^,;]*)*),(.*)$/su.exec(url);
  if (data) {
    const type = data[1] || attachment.media_type;
    const payload = data[3] ?? "";
    const bytes = data[2]?.split(";").includes("base64")
      ? Uint8Array.from(atob(payload), (character) => character.charCodeAt(0))
      : new TextEncoder().encode(decodeURIComponent(payload));
    return new File([bytes], attachment.filename, { type });
  }
  const blob = await (await fetch(url)).blob();
  return new File([blob], attachment.filename, { type: blob.type || attachment.media_type });
}

/**
 * The chat's images in one viewer (context-rail spec §2.4). Previous and next
 * step through every image sent in this chat, not just one message; "Add to
 * question" puts the image back in the composer as an attachment without
 * sending. Thumbnails in the thread and in "In this chat" open it.
 */
export function ChatLightboxProvider({ messages, onRead, onAddToQuestion, children }: {
  messages: readonly ChatMessage[];
  onRead: (messageId: string, attachmentId: string) => Promise<string>;
  /** Attaches the image to the next question; absent where nothing can be asked. */
  onAddToQuestion?: (file: File) => void;
  children: ReactNode;
}) {
  useTranslation("chat");
  const images = useMemo<ChatImage[]>(() => messages.flatMap((message) => (message.attachments ?? []).filter(isImage)
    .map((attachment) => ({ key: imageKey(message.id, attachment.id), messageId: message.id, attachment, sentAt: message.created_at }))), [messages]);
  const [states, setStates] = useState<Record<string, ImageState>>({});
  const requested = useRef(new Set<string>());
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [addFailed, setAddFailed] = useState(false);
  // Closing for "Add to question" leaves focus in the composer, not on the thumbnail.
  const toComposer = useRef<File | null>(null);

  const load = useCallback((messageId: string, attachment: ChatAttachment) => {
    const key = imageKey(messageId, attachment.id);
    if (attachment.url || requested.current.has(key)) return;
    requested.current.add(key);
    void onRead(messageId, attachment.id)
      .then((url) => setStates((current) => ({ ...current, [key]: { url, failed: false } })))
      .catch(() => setStates((current) => ({ ...current, [key]: { failed: true } })));
  }, [onRead]);
  const image = useCallback((messageId: string, attachment: ChatAttachment): ImageState => {
    const state = states[imageKey(messageId, attachment.id)];
    return { url: attachment.url ?? state?.url, failed: !attachment.url && Boolean(state?.failed) };
  }, [states]);
  // Closing returns focus to the thumbnail that opened the viewer (WebKit does not focus a clicked button).
  const opener = useRef<HTMLElement | null>(null);
  const open = useCallback((messageId: string, attachmentId: string, from?: HTMLElement) => {
    opener.current = from ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    setAddFailed(false);
    setOpenKey(imageKey(messageId, attachmentId));
  }, []);
  const value = useMemo(() => ({ load, image, open }), [load, image, open]);

  const index = openKey === null ? -1 : images.findIndex((item) => item.key === openKey);
  const current = index < 0 ? undefined : images[index];
  // The open image and its neighbours read ahead, so stepping never waits on a blank stage.
  useEffect(() => {
    if (index < 0) return;
    for (const offset of [0, 1, -1]) {
      const near = images[(index + offset + images.length) % images.length];
      if (near) load(near.messageId, near.attachment);
    }
  }, [images, index, load]);
  const step = (by: number) => {
    if (index < 0 || images.length < 2) return;
    setAddFailed(false);
    setOpenKey(images[(index + by + images.length) % images.length]!.key);
  };
  const url = current ? image(current.messageId, current.attachment).url : undefined;
  const addToQuestion = async () => {
    if (!current || !url || !onAddToQuestion) return;
    setAdding(true);
    try {
      toComposer.current = await attachmentFile(url, current.attachment);
      setOpenKey(null);
    } catch {
      setAddFailed(true);
    } finally {
      setAdding(false);
    }
  };
  const sent = current ? Date.parse(current.sentAt) : Number.NaN;
  const several = images.length > 1;

  return <ChatLightboxContext.Provider value={value}>
    {children}
    <Dialog open={current !== undefined} onOpenChange={(next) => { if (!next) setOpenKey(null); }}>
      {current ? <DialogContent className="lightbox" aria-describedby={undefined} tabIndex={-1}
        // The viewer itself takes focus (arrows step from anywhere in it); a ring on its first button read as a selection.
        onOpenAutoFocus={(event) => { event.preventDefault(); (event.currentTarget as HTMLElement | null)?.focus({ preventScroll: true }); }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          const file = toComposer.current;
          toComposer.current = null;
          if (file) onAddToQuestion?.(file);
          else opener.current?.focus();
          opener.current = null;
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowLeft" && several) { event.preventDefault(); step(-1); }
          if (event.key === "ArrowRight" && several) { event.preventDefault(); step(1); }
        }}>
        <DialogTitle className="sr-only">{t("chatLightbox.label")}</DialogTitle>
        <div className="lightbox-stage">
          {url ? <img src={url} alt={current.attachment.filename} /> : <span className="lightbox-placeholder" aria-hidden="true" />}
        </div>
        <div className="lightbox-caption">
          {several ? <Button type="button" variant="ghost" size="icon" aria-label={t("chatLightbox.previous")} onClick={() => step(-1)}><ChevronLeftIcon /></Button> : null}
          <p>
            <strong title={current.attachment.filename}>{current.attachment.filename}</strong>
            <span>{[
              fileSize(current.attachment.size_bytes),
              Number.isFinite(sent) ? t("chatLightbox.sentOn", { date: formatDate(sent, { year: undefined, month: "short", day: "numeric" }) }) : "",
              several ? t("chatLightbox.position", { index: formatNumber(index + 1), total: formatNumber(images.length) }) : "",
            ].filter(Boolean).join(" · ")}</span>
          </p>
          {addFailed ? <span className="lightbox-error" role="alert">{t("chatLightbox.addFailed")}</span> : null}
          {url ? <Button type="button" variant="ghost" size="sm" asChild><a href={url} download={current.attachment.filename}><DownloadIcon aria-hidden="true" />{t("chatLightbox.download")}</a></Button> : null}
          {onAddToQuestion ? <Button type="button" variant="ghost" size="sm" data-lightbox-add disabled={!url || adding} onClick={() => void addToQuestion()}>
            <PlusIcon aria-hidden="true" />{t("chatLightbox.addToQuestion")}
          </Button> : null}
          {several ? <Button type="button" variant="ghost" size="icon" aria-label={t("chatLightbox.next")} onClick={() => step(1)}><ChevronRightIcon /></Button> : null}
        </div>
      </DialogContent> : null}
    </Dialog>
  </ChatLightboxContext.Provider>;
}
