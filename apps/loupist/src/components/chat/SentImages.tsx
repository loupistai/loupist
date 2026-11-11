import i18n, { useTranslation } from "@/i18n";
import { useEffect } from "react";
import { FileIcon } from "lucide-react";
import type { ChatMessage } from "../../types";
import { useChatLightbox } from "./ChatLightbox";

const t = i18n.getFixedT(null, "chat");

/**
 * Images the user sent, as small thumbnails above the message (context-rail
 * spec §2.3). Each opens the chat's lightbox, which steps through every image
 * in the chat; outside a chat the thumbnails only show.
 */
export function SentImages({ message }: { message: ChatMessage }) {
  useTranslation("chat");
  const lightbox = useChatLightbox();
  const images = (message.attachments ?? []).filter((attachment) => attachment.media_type.startsWith("image/"));
  const imageIds = images.map((image) => image.id).join(",");
  useEffect(() => {
    for (const image of images) lightbox?.load(message.id, image);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reads once per image; the lightbox keeps what it read.
  }, [imageIds, message.id, lightbox?.load]);
  if (!images.length) return null;
  return <div className="sent-images" aria-label={t("chatMessageCard.messageAttachments")}>
    {images.map((image) => {
      const state = lightbox?.image(message.id, image) ?? { url: image.url, failed: false };
      const preview = state.url ? <img src={state.url} alt="" /> : state.failed ? <FileIcon aria-hidden="true" /> : <span aria-hidden="true" />;
      const title = state.failed ? `${image.filename} · ${t("chatMessageCard.previewUnavailable")}` : image.filename;
      return lightbox
        ? <button key={image.id} type="button" className="sent-image" title={title}
          aria-label={t("chatLightbox.open", { filename: image.filename })} onClick={(event) => lightbox.open(message.id, image.id, event.currentTarget)}>{preview}</button>
        : <span key={image.id} className="sent-image" title={title} role="img" aria-label={image.filename}>{preview}</span>;
    })}
  </div>;
}
