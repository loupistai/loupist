import { useCallback, useRef, useState } from "react";
import { useTranslation } from "@/i18n";

/** Keep the last failed browser address available at the control that opened it. */
export function useExternalLink(openUrl: (url: string) => void | Promise<void>) {
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  const attempt = useRef(0);
  const clear = useCallback(() => {
    attempt.current += 1;
    setFailedUrl(null);
  }, []);
  const open = useCallback(async (url: string) => {
    clear();
    const current = attempt.current;
    try {
      await openUrl(url);
    } catch {
      if (attempt.current === current) setFailedUrl(url);
    }
  }, [clear, openUrl]);
  return { open, failedUrl, clear };
}

export function ExternalLinkError({ url }: { url: string | null }) {
  const { t } = useTranslation("common");
  return url ? <span className="inline-error external-link-error" role="alert">
    <span>{t("externalLink.couldNotOpen")}</span>
    <span className="external-link-url">{url}</span>
  </span> : null;
}
