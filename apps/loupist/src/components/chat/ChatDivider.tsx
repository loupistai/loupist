import i18n, { useTranslation } from "@/i18n";
import { useEffect, useRef } from "react";
import type { Divider } from "../../chat/dividers";

const t = i18n.getFixedT(null, "chat");

/**
 * A full-width rule with a centered label between messages (chat spec X5):
 * the day when it changes, or "New since you left". The latter calls
 * `onPassed` once the reader has seen it and scrolled past it.
 */
export function ChatDivider({ divider, onPassed }: { divider: Divider; onPassed?: () => void }) {
  useTranslation("chat");
  const element = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const node = element.current;
    if (divider.kind !== "new" || !onPassed || !node || typeof IntersectionObserver === "undefined") return undefined;
    let shown = false;
    const observer = new IntersectionObserver(([entry]) => {
      if (!entry) return;
      if (entry.isIntersecting) { shown = true; return; }
      // Seen, then scrolled up out of view: the reader is past the new answers' start.
      if (shown && entry.boundingClientRect.bottom <= (entry.rootBounds?.top ?? 0)) onPassed();
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [divider.kind, onPassed]);
  const label = divider.kind === "new"
    ? divider.day ? `${divider.day} · ${t("chatDivider.newSinceYouLeft")}` : t("chatDivider.newSinceYouLeft")
    : divider.label;
  return <div ref={element} className="chat-divider" data-kind={divider.kind} role="separator" aria-label={label}>
    <span>{label}</span>
  </div>;
}
