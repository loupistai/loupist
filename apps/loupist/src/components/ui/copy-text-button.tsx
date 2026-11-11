import i18n, { useTranslation } from "@/i18n";
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { CheckIcon, CopyIcon } from "lucide-react";

export function CopyTextButton({ text, label }: { text: string; label: string }) {
  useTranslation();
  const { state: status, copy } = useCopyFeedback(text);
  return <span className="execution-copy-control">
    <button type="button" className="execution-copy-button" aria-label={status === "copied" ? i18n.t("common:copyTextButton.copiedLabel", { label }) : i18n.t("common:copyTextButton.copy", { label: label })} title={status === "copied" ? i18n.t("common:copyTextButton.copied") : i18n.t("common:copyTextButton.copy", { label: label })}
      onClick={() => { void copy(text); }}>
      {status === "copied" ? <CheckIcon aria-hidden="true" /> : <CopyIcon aria-hidden="true" />}
    </button>
    {status === "failed" ? <span role="alert">{i18n.t("common:copyTextButton.selectTheTextToCopyIt")}</span> : null}
  </span>;
}
