import i18n, { useTranslation } from "@/i18n";
import { XIcon } from "lucide-react";
import { Button } from "../ui/button";
import type { ConfigSnapshot } from "../../types";

const t = i18n.getFixedT(null, "chat");

export function ModelNoticeLine({ notice, onChange, onDismiss }: { notice: NonNullable<ConfigSnapshot["defaultModelChange"]>; onChange?: () => void; onDismiss: () => void }) {
  useTranslation("chat");
  return <div className="model-notice-line" role="status" data-model-notice="default-change">
    <p>
      {notice.current
        ? notice.previous ? t("modelNotice.changed", { current: notice.current.name, previous: notice.previous.name }) : t("modelNotice.available", { current: notice.current.name })
        : t("chatPendingQueue.unavailableModel")}
      {onChange ? <> <button type="button" className="model-notice-change" onClick={onChange}>{t("modelNotice.change")}</button></> : null}
    </p>
    <Button type="button" variant="ghost" size="icon-xs" aria-label={t("modelNotice.dismiss")} title={t("modelNotice.dismiss")} onClick={onDismiss}><XIcon aria-hidden="true" /></Button>
  </div>;
}
