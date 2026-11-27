import i18n from "../i18n";
import { formatNumber } from "../i18n/format";
import type { GraphProcessingView, KnowledgeCatalog } from "../types";
import { worldReadinessLabels, type WorldReadinessStatus } from "./worldReadiness";

type Readiness = GraphProcessingView["readiness"];
type WaitingReason = Exclude<WorldReadinessStatus, "ready" | "recovering"> | "awaiting_provider" | "no_account" | "paused_budget" | "failed_before_mutation" | "reconcile_required";

export type WorldProcessing = {
  /** A wait on the request limit names the Sources stopped there, by record: what "Continue all" continues. */
  activity: { kind: "building"; sourceCount: number } | { kind: "waiting"; reason: WaitingReason; stopped?: string[] } | { kind: "idle" };
  readiness: Readiness | null;
  pending: boolean;
};

export const isMapProcessing = (status: string | undefined) => status === "queued" || status === "running";

function blockedReadiness(readiness: Readiness | null): WaitingReason | undefined {
  if (!readiness || readiness.status === "ready" || readiness.status === "recovering") return undefined;
  // The host words its waiting by code: no account at all, or a new Loupist awaiting its default model.
  if (readiness.note === "no_account" || readiness.note === "awaiting_provider") return readiness.note;
  return readiness.status;
}

export function sourceDisplayStatus(status: string | undefined, readiness: Readiness | null | undefined) {
  if (status === "paused" && readiness?.status === "disabled") return "disabled";
  if (status !== "queued" || readiness === undefined) return status;
  if (!readiness) return undefined;
  return blockedReadiness(readiness) ?? status;
}

export function processingSourceCount(catalog: KnowledgeCatalog | null, readiness?: Readiness | null) {
  return catalog?.sources.reduce((total, source) =>
    total + source.objects.filter(object => isMapProcessing(sourceDisplayStatus(object.processing?.status, readiness))).length, 0) ?? 0;
}

export function worldProcessing(catalog: KnowledgeCatalog | null, readiness: Readiness | null): WorldProcessing {
  const statuses = new Set(catalog?.sources.flatMap(record => record.objects.map(object => object.processing?.status)));
  const pending = statuses.has("queued") || statuses.has("running") || readiness?.status === "recovering";
  if (statuses.has("running") || readiness?.status === "recovering" || (statuses.has("queued") && readiness?.status === "ready")) {
    return { activity: { kind: "building", sourceCount: processingSourceCount(catalog, readiness) }, readiness, pending };
  }
  const blocked = statuses.has("queued") || statuses.has("paused") ? blockedReadiness(readiness) : undefined;
  const sourceReason = (["reconcile_required", "paused_budget", "failed_before_mutation", "paused"] as const).find(status => statuses.has(status));
  const reason = blocked ?? sourceReason;
  if (reason === "paused_budget") return { activity: { kind: "waiting", reason, stopped: stoppedSources(catalog, readiness) }, readiness, pending };
  return { activity: reason ? { kind: "waiting", reason } : { kind: "idle" }, readiness, pending };
}

/** The Sources, by record, whose World building stopped at the request limit. */
export function stoppedSources(catalog: KnowledgeCatalog | null, readiness: Readiness | null | undefined) {
  return catalog?.sources
    .filter(record => record.objects.some(object => sourceDisplayStatus(object.processing?.status, readiness) === "paused_budget"))
    .map(record => record.recordId) ?? [];
}

/** What World's own status line says while it waits: how many Sources stopped at the limit, or the reason. */
export function waitingLabel(activity: Extract<WorldProcessing["activity"], { kind: "waiting" }>) {
  const count = activity.stopped?.length ?? 0;
  if (activity.reason === "paused_budget" && count) return i18n.t("knowledge:sourceRetry.pausedAtLimit", { count, formattedCount: formatNumber(count) });
  return sourceProcessingLabel(activity.reason);
}

/** A Source stop the Library and the chat status both explain. `failure` wins over the status. */
export type StoppedSource = {
  status?: string | null;
  failure?: { kind: string } | null;
  retryable?: boolean | null;
};

const MODEL_OUTPUT = new Set(["invalid_json", "schema_mismatch", "incomplete_response", "output_size", "content_type"]);

/**
 * The one line for a stopped Source. A recorded `failure.kind` decides it;
 * otherwise the status is the cause. World full keeps the Source and does not
 * read it in.
 */
export function stoppedSourceLine(processing?: StoppedSource | null) {
  const kind = processing?.failure?.kind;
  if (kind === "world_full") return i18n.t("knowledge:sourceWorkspace.worldFull");
  if (kind && MODEL_OUTPUT.has(kind)) return i18n.t("knowledge:sourceWorkspace.modelReplyUnusable");
  if (kind === "provider_error") return i18n.t("knowledge:sourceWorkspace.providerError");
  return sourceProcessingLabel(processing?.status ?? undefined);
}

export type SourceHold = "allowance" | "daily_allowance" | "plan_limit" | "out_of_credit";

const HOLDS = new Set<SourceHold>(["allowance", "daily_allowance", "plan_limit", "out_of_credit"]);

function holdName(value: unknown): SourceHold | undefined {
  return typeof value === "string" && HOLDS.has(value as SourceHold) ? value as SourceHold : undefined;
}

/**
 * Why a Source is waiting instead of reading. The host puts this on `hold`
 * as `{ cause, provider }` while the job stays queued. A status string is the
 * same wait when no job has been read yet.
 */
export function sourceHold(processing?: StoppedSource | null): SourceHold | undefined {
  const raw = (processing as { hold?: unknown } | null | undefined)?.hold;
  const named = holdName(raw) ?? holdName(holdCause(raw));
  if (named) return named;
  return holdName(processing?.status);
}

function holdCause(value: unknown): unknown {
  if (!value || typeof value !== "object") return undefined;
  const record = value as { cause?: unknown; reason?: unknown };
  return record.cause ?? record.reason;
}

/** The one line for a Source waiting on an allowance or a plan limit. */
export function sourceHoldLine(hold: SourceHold) {
  return hold === "plan_limit" || hold === "out_of_credit"
    ? i18n.t("knowledge:sourceWorkspace.planLimitReading")
    : i18n.t("knowledge:sourceWorkspace.waitingForTomorrow");
}

/** Try again only when the host says this stop can run again. A full World has no action. */
export function sourceRetryOffered(processing?: StoppedSource | null) {
  if (processing?.retryable !== true || processing.failure?.kind === "world_full") return false;
  const status = processing.status ?? "";
  return !!status && !isMapProcessing(status) && status !== "completed" && status !== "price_data";
}

export function sourceProcessingLabel(status?: string) {
  switch (status) {
    case "queued": case "running": return i18n.t("knowledge:sourceWorkspace.worldIsUpdating");
    case "completed": return i18n.t("knowledge:sourceWorkspace.worldUpdated");
    case "failed_extraction": return i18n.t("knowledge:sourceWorkspace.modelReplyUnusable");
    case "provider_error": return i18n.t("knowledge:sourceWorkspace.providerError");
    case "awaiting_provider": return i18n.t("knowledge:worldProcessingSettings.waitingForAProvider");
    case "no_account": return i18n.t("knowledge:sourceWorkspace.worldWaitingForAccount");
    case "provider_disconnected": return i18n.t("knowledge:sourceWorkspace.worldWaitingForModel");
    case "paused": return i18n.t("knowledge:sourceWorkspace.worldProcessingPaused");
    case "paused_budget": return i18n.t("knowledge:sourceWorkspace.worldProcessingPausedAtAllowance");
    case "failed_before_mutation": return i18n.t("knowledge:sourceWorkspace.worldProcessingFailed");
    case "reconciliation_required": case "reconcile_required": return i18n.t("knowledge:sourceWorkspace.worldUpdateNeedsReview");
    case "model_required": case "runtime_unavailable": case "model_unavailable": return worldReadinessLabels()[status];
    case "interrupted": return i18n.t("knowledge:sourceWorkspace.worldProcessingInterrupted");
    case "timed_out": return i18n.t("knowledge:sourceWorkspace.worldProcessingTimedOut");
    case "usage_uncertain": return i18n.t("knowledge:sourceWorkspace.worldProcessingUsageUncertain");
    case "reconciled_without_retry": return i18n.t("knowledge:sourceWorkspace.worldProcessingIncomplete");
    case "cancelled_before_start": return i18n.t("knowledge:sourceWorkspace.worldProcessingCancelled");
    case "unsupported": case "no_text": return i18n.t("knowledge:sourceWorkspace.noTextForWorldProcessing");
    case "price_data": return i18n.t("knowledge:sourceWorkspace.priceDataNotAddedToWorld");
    case "disabled": case "not_configured": return i18n.t("knowledge:sourceWorkspace.worldProcessingIsOff");
    default: return i18n.t("knowledge:sourceWorkspace.sourceSaved");
  }
}
