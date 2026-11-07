import { useTranslation } from "@/i18n";

/** Bordered principle quote shared by answer feedback and the Memory row. */
export function SavedPrinciple({ statement, undone = false, pending = false, showLabel = true, onUndo, onView, className }: {
  statement: string;
  undone?: boolean;
  pending?: boolean;
  showLabel?: boolean;
  onUndo?: () => void;
  /** Absent where nothing can open the page: the quote stands without a View control. */
  onView?: () => void;
  className?: string;
}) {
  const { t } = useTranslation("chat");
  return (
    <div className={["saved-principle", className].filter(Boolean).join(" ")} role="status">
      {showLabel ? <p className="saved-principle-label">{t("principles.saved")}</p> : null}
      <blockquote className="saved-principle-quote">
        <p>{undone ? t("principles.undone") : statement}</p>
      </blockquote>
      <div className="message-actions">
        {!undone && onUndo ? <button type="button" disabled={pending} onClick={onUndo}>{t("principles.undo")}</button> : null}
        {onView ? <button type="button" onClick={onView}>{t("principles.view")}</button> : null}
      </div>
    </div>
  );
}
