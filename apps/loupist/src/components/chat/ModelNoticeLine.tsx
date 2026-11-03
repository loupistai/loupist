import i18n, { useTranslation } from "@/i18n";
import { XIcon } from "lucide-react";
import { Button } from "../ui/button";
import type { ModelNotice } from "../../types";

const t = i18n.getFixedT(null, "chat");

/**
 * One meta line above the composer when the model Chat uses changed under the
 * user (W17.5): "Now using {model}. Change" or "{model} is no longer offered.
 * Chat uses Auto now." Shown once; gone on send or ×.
 */
export function ModelNoticeLine({ notice, onChange, onDismiss }: { notice: ModelNotice; onChange?: () => void; onDismiss: () => void }) {
  useTranslation("chat");
  return <div className="model-notice-line" role="status" data-model-notice={notice.kind}>
    <p>
      {notice.kind === "now_using" ? t("modelNotice.nowUsing", { model: notice.model }) : t("modelNotice.removed", { model: notice.model })}
      {notice.kind === "now_using" && onChange ? <> <button type="button" className="model-notice-change" onClick={onChange}>{t("modelNotice.change")}</button></> : null}
    </p>
    <Button type="button" variant="ghost" size="icon-xs" aria-label={t("modelNotice.dismiss")} title={t("modelNotice.dismiss")} onClick={onDismiss}><XIcon aria-hidden="true" /></Button>
  </div>;
}
