import { useEffect, useRef, useState } from "react";
import { useTranslation } from "../../i18n";
import { errorMessage } from "../../lib/errors";
import type { GraphJobView } from "../../types";
import { Button } from "../ui/button";

/**
 * Asks the host to give one Source whose Map building stopped (`paused_budget`) one more
 * allowance of model requests. The host decides; its refusal is shown as it gave it.
 */
export function useSourceRetry(retry: () => Promise<GraphJobView>, onRetried: (job: GraphJobView) => void) {
  const { t } = useTranslation("knowledge");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const live = useRef(true);
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
  const run = async () => {
    if (pending) return;
    setPending(true); setError(null);
    try {
      const job = await retry();
      if (live.current) setPending(false);
      onRetried(job);
    } catch (cause) {
      if (!live.current) return;
      setPending(false);
      setError(errorMessage(cause, t("sourceRetry.couldNotTryAgain")));
    }
  };
  return { pending, error, run };
}

export function SourceRetry({ retry, onRetried }: { retry: () => Promise<GraphJobView>; onRetried: (job: GraphJobView) => void }) {
  const { t } = useTranslation("knowledge");
  const { pending, error, run } = useSourceRetry(retry, onRetried);
  return <div className="source-retry">
    <Button type="button" variant="ghost" size="xs" disabled={pending} onClick={() => void run()}>{pending ? t("sourceRetry.tryingAgain") : t("sourceRetry.tryAgain")}</Button>
    <span className="knowledge-hint">{t("sourceRetry.usesOneMoreAllowance")}</span>
    {error ? <p role="alert">{error}</p> : null}
  </div>;
}
