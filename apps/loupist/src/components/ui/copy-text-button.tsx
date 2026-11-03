import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState } from "react";
import { CheckIcon, CopyIcon } from "lucide-react";

export function CopyTextButton({ text, label }: { text: string; label: string }) {
  useTranslation();
  const [status, setStatus] = useState<"idle" | "copied" | "failed">("idle");
  useEffect(() => setStatus("idle"), [text]);
  return <span className="execution-copy-control">
    <button type="button" className="execution-copy-button" aria-label={status === "copied" ? i18n.t("common:copyTextButton.copiedLabel", { label }) : i18n.t("common:copyTextButton.copy", { label: label })} title={status === "copied" ? i18n.t("common:copyTextButton.copied") : i18n.t("common:copyTextButton.copy", { label: label })}
      onClick={() => { void (async () => { try { await navigator.clipboard.writeText(text); setStatus("copied"); } catch { setStatus("failed"); } })(); }}>
      {status === "copied" ? <CheckIcon aria-hidden="true" /> : <CopyIcon aria-hidden="true" />}
    </button>
    {status === "failed" ? <span role="alert">{i18n.t("common:copyTextButton.selectTheTextToCopyIt")}</span> : null}
  </span>;
}
