import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { LoupistBridge } from "../../types";
import type { LearningReviewFinishedEvent } from "../../contracts/knowledge";
import { useTranslation } from "../../i18n";
import { Button } from "../ui/button";
import { glowLearnedMethod } from "./learningState";
import "../../styles/map.css";

export function useLearningNotifications(bridge: LoupistBridge, activeMapLoupistId: string | undefined, onChanged: () => void) {
  const [toasts, setToasts] = useState<(LearningReviewFinishedEvent & { title: string })[]>([]);
  const current = useRef({ activeMapLoupistId, onChanged });
  current.current = { activeMapLoupistId, onChanged };
  useEffect(() => {
    let mounted = true;
    const seen = new Set<string>();
    const unsubscribe = bridge.onLearningReviewFinished(event => {
      const { review, loupistId } = event;
      if (seen.has(review.reviewId)) return;
      seen.add(review.reviewId);
      current.current.onChanged();
      if ((review.status !== "created" && review.status !== "refined") || !review.lensRecordId) return;
      if (current.current.activeMapLoupistId === loupistId) glowLearnedMethod(bridge, loupistId, review.lensRecordId);
      setToasts(items => [...items, { ...event, title: "" }]);
      const updateTitle = (title: string) => { if (mounted) setToasts(items => items.map(item => item.review.reviewId === review.reviewId ? { ...item, title } : item)); };
      void bridge.knowledgeLensRead({ loupistId, recordId: review.lensRecordId, includeNavigation: false }).then(lens => {
        updateTitle(lens.title);
      }).catch(() => {
        // A catalog read can still resolve a title when the exact document is unavailable.
        void bridge.knowledgeCatalog({ loupistId }).then(catalog => {
          const lens = catalog.lenses.find(item => item.recordId === review.lensRecordId);
          updateTitle(lens?.title ?? "");
        }).catch(() => {});
      });
    });
    return () => { mounted = false; unsubscribe(); };
  }, [bridge]);
  return { toasts, dismiss: (reviewId: string) => setToasts(items => items.filter(item => item.review.reviewId !== reviewId)) };
}
function useMapCaption() {
  const [slot, setSlot] = useState<Element | null>(null);
  useEffect(() => {
    const find = () => document.querySelector("[data-map-caption]");
    setSlot(find());
    const observer = new MutationObserver(() => {
      const next = find();
      setSlot(current => current === next ? current : next);
    });
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);
  return slot;
}
export function LearningToasts({ notifications, onView }: { notifications: ReturnType<typeof useLearningNotifications>; onView: (loupistId: string, recordId: string) => void | Promise<void> }) {
  const { t } = useTranslation("knowledge");
  const caption = useMapCaption();
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();
  const view = async (item: typeof notifications.toasts[number]) => {
    setBusy(item.review.reviewId); setError(undefined);
    try { await onView(item.loupistId, item.review.lensRecordId!); notifications.dismiss(item.review.reviewId); }
    catch { setError(item.review.reviewId); }
    finally { setBusy(undefined); }
  };
  const toasts = <div className="learning-toasts" aria-live="polite">{notifications.toasts.filter(item => item.title).map(item => <div className="learning-toast" role="status" key={item.review.reviewId}>
    <p>{t(item.review.status === "refined" ? "learning.refinedToast" : "learning.createdToast", { title: item.title })}</p>
    <Button variant="link" disabled={busy === item.review.reviewId} onClick={() => void view(item)}>{t("learning.view")}</Button>
    {error === item.review.reviewId ? <p role="alert">{t("learning.openError")}</p> : null}
    <Button variant="ghost" onClick={() => notifications.dismiss(item.review.reviewId)}>{t("learning.dismiss")}</Button>
  </div>)}</div>;
  return caption ? createPortal(toasts, caption) : toasts;
}
