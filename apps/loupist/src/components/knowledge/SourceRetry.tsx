import { useEffect, useRef, useState } from "react";
import i18n, { useTranslation } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { errorMessage } from "../../lib/errors";
import type { GraphJobView, LoupistBridge } from "../../types";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";

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

/**
 * "Continue all" on World's status line: one confirmed step that gives every Source stopped at the
 * request limit the same retry as its own Try again. The confirmation names the Sources counted when
 * it opened and the most model requests they can use (Sources × the allowance saved now); only those
 * Sources, under that allowance, run, and nothing runs without it. A Source the host's retry refuses
 * is skipped; when none can continue the dialog says so.
 */
export function ContinueStoppedSources({ bridge, loupistId, recordIds, onContinued }: {
  bridge: Pick<LoupistBridge, "knowledgeGraphSettingsGet" | "knowledgeGraphRetrySources">;
  loupistId: string;
  recordIds: string[];
  onContinued?: () => void;
}) {
  const { t } = useTranslation("knowledge");
  const [confirming, setConfirming] = useState<{ recordIds: string[]; maxLlmCalls: number } | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const live = useRef(true);
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
  const close = () => { if (!pending) { setConfirming(null); setError(null); } };
  const open = async () => {
    if (pending) return;
    const counted = [...recordIds];
    setPending(true); setError(null);
    try {
      const view = await bridge.knowledgeGraphSettingsGet({ loupistId });
      if (live.current) setConfirming({ recordIds: counted, maxLlmCalls: view.settings.maxLlmCalls });
    } catch (cause) {
      if (live.current) setError(errorMessage(cause, t("sourceRetry.couldNotContinue")));
    } finally {
      if (live.current) setPending(false);
    }
  };
  const run = async () => {
    if (!confirming || pending) return;
    setPending(true); setError(null);
    try {
      const result = await bridge.knowledgeGraphRetrySources({ loupistId, ...confirming });
      if (!live.current) return;
      setPending(false);
      if (result.continued === 0) { setError(t("sourceRetry.noneContinued")); return; }
      setConfirming(null);
      onContinued?.();
    } catch (cause) {
      if (!live.current) return;
      setPending(false);
      setError(errorMessage(cause, t("sourceRetry.couldNotContinue")));
    }
  };
  const count = confirming?.recordIds.length ?? 0;
  const requests = count * (confirming?.maxLlmCalls ?? 0);
  return <>
    <Button variant="link" disabled={pending && !confirming} onClick={() => void open()}>{t("sourceRetry.continueAll")}</Button>
    {error && !confirming ? <span role="alert">{error}</span> : null}
    <Dialog open={confirming !== null} onOpenChange={next => { if (!next) close(); }}>
      <DialogContent size="sm" data-continue-stopped-sources>
        <DialogHeader>
          <DialogTitle>{t("sourceRetry.continueAllTitle", { count, formattedCount: formatNumber(count) })}</DialogTitle>
          <DialogDescription>{t("sourceRetry.continueAllCost", { count: requests, formattedCount: formatNumber(requests) })}</DialogDescription>
        </DialogHeader>
        {error ? <p role="alert">{error}</p> : null}
        <DialogFooter>
          <Button variant="outline" disabled={pending} onClick={close}>{i18n.t("onboarding:appDialogs.cancel")}</Button>
          <Button disabled={pending} onClick={() => void run()}>{pending ? t("sourceRetry.continuing") : t("sourceRetry.continue")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </>;
}
