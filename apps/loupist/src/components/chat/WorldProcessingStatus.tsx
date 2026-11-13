import i18n, { useTranslation } from "@/i18n";
import { useEffect, useRef, useState } from "react";
import type { GraphJobView } from "../../types";
import { useSourceRetry } from "../knowledge/SourceRetry";

const t = i18n.getFixedT(null, "chat");

const labels = (): Record<string, string> => ({
  queued: t("worldProcessingStatus.queued"),
  running: t("worldProcessingStatus.running"),
  completed: t("worldProcessingStatus.completed"),
  paused: t("worldProcessingStatus.paused"),
  paused_budget: t("worldProcessingStatus.paused_budget"),
  failed_before_mutation: t("worldProcessingStatus.failed_before_mutation"),
  reconcile_required: t("worldProcessingStatus.reconcile_required"),
  reconciled_without_retry: t("worldProcessingStatus.reconciled_without_retry"),
  cancelled_before_start: t("worldProcessingStatus.cancelled_before_start"),
  no_text: t("worldProcessingStatus.no_text"),
  disabled: t("worldProcessingStatus.disabled"),
  not_configured: t("worldProcessingStatus.not_configured"),
  saved: t("worldProcessingStatus.saved"),
});
const parked = (status: string) => status === "paused" || status === "paused_budget";
const pending = (status: string) => status === "queued" || status === "running";

/**
 * Reads a committed job only; polling never schedules or retries extraction. Only the user's
 * "Try again" on a Source that used up its allowance asks the host for one more, and the
 * status then follows the job the host returned.
 */
export function WorldProcessingStatus({ active = true, jobId: committedJobId, initialStatus: committedStatus, readJob, onChanged, retrySource }: {
  active?: boolean;
  jobId?: string | null;
  initialStatus?: string | null;
  readJob?: (jobId: string) => Promise<GraphJobView>;
  onChanged?: () => void;
  retrySource?: () => Promise<GraphJobView>;
}) {
  useTranslation("chat");
  const [followed, setFollowed] = useState<{ jobId: string; status: string } | null>(null);
  const jobId = followed?.jobId ?? committedJobId;
  const initialStatus = followed?.status ?? committedStatus;
  const [status, setStatus] = useState(initialStatus ?? "saved");
  const [failed, setFailed] = useState(false);
  const [retry, setRetry] = useState(0);
  const callbacks = useRef({ readJob, onChanged });
  callbacks.current = { readJob, onChanged };
  const canRead = Boolean(readJob);
  useEffect(() => {
    if (!active) return;
    setStatus(initialStatus ?? "saved");
    setFailed(false);
    if (!jobId || !canRead || (!pending(initialStatus ?? "") && !parked(initialStatus ?? "") && initialStatus !== "reconcile_required")) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = async () => {
      try {
        const job = await callbacks.current.readJob!(jobId);
        if (cancelled) return;
        setStatus(job.status);
        setFailed(false);
        if (pending(job.status)) timer = setTimeout(() => void refresh(), 2_000);
        else if (job.status !== initialStatus) callbacks.current.onChanged?.();
      } catch {
        if (!cancelled) setFailed(true);
      }
    };
    void refresh();
    return () => { cancelled = true; if (timer) clearTimeout(timer); };
  }, [active, jobId, initialStatus, canRead, retry]);
  if (!initialStatus && !jobId) return null;
  return <small role="status">
    {failed ? t("worldProcessingStatus.sourceSavedCouldNotRefreshWorldStatus") : labels()[status] ?? t("worldProcessingStatus.sourceSavedCheckWorldForProcessingDetails")}
    {failed || ((status === "reconcile_required" || parked(status)) && jobId && readJob) ? <button type="button" onClick={() => setRetry((value) => value + 1)}>{t("worldProcessingStatus.refreshStatus")}</button> : null}
    {!failed && status === "paused_budget" && retrySource ? <RetryStoppedSource key={jobId ?? "retry"} retry={retrySource} onRetried={(job) => { setFollowed({ jobId: job.jobId, status: job.status }); callbacks.current.onChanged?.(); }} /> : null}
  </small>;
}

function RetryStoppedSource({ retry, onRetried }: { retry: () => Promise<GraphJobView>; onRetried: (job: GraphJobView) => void }) {
  const { t: tk } = useTranslation("knowledge");
  const { pending: retrying, error, run } = useSourceRetry(retry, onRetried);
  return <span className="world-status-retry">
    <button type="button" disabled={retrying} onClick={() => void run()}>{retrying ? tk("sourceRetry.tryingAgain") : tk("sourceRetry.tryAgain")}</button>
    <span>{tk("sourceRetry.usesOneMoreAllowance")}</span>
    {error ? <span role="alert">{error}</span> : null}
  </span>;
}
