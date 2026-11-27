import i18n from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { providerShortName } from "../../chat/providerNames";
import type { Bucket, Hold } from "../../contracts/generated";
import type { OpenRouterQuota } from "../../contracts/generated/providers/openrouter_quota";

const counts = (bucket: Bucket) => ({ used: formatNumber(bucket.used), limit: formatNumber(bucket.limit) });

const money = (value: number) => formatNumber(value, { style: "currency", currency: "USD", currencyDisplay: "narrowSymbol" });

/** Background allowance. Off, the 80% warning, and the hold use the scheduler's sentences. */
export function backgroundLine(bucket: Bucket) {
  if (bucket.state === "off") return i18n.t("settings:usage.allowance.off");
  if (bucket.state === "reached") return i18n.t("settings:usage.allowance.waiting");
  if (bucket.state === "warning") return i18n.t("settings:usage.allowance.backgroundWarning", counts(bucket));
  return i18n.t("settings:usage.allowance.backgroundOk", counts(bucket));
}

/** The separate allowance for reading Sources saved in Chat. */
export function sourcesLine(bucket: Bucket) {
  if (bucket.state === "off") return i18n.t("settings:usage.allowance.off");
  if (bucket.state === "reached") return i18n.t("settings:usage.allowance.waiting");
  if (bucket.state === "warning") return i18n.t("settings:usage.allowance.sourcesWarning", counts(bucket));
  return i18n.t("settings:usage.allowance.sourcesOk", counts(bucket));
}

export function worldShareLine(share: { used: number; limit: number }) {
  return i18n.t("settings:usage.allowance.worldShare", { used: formatNumber(share.used), limit: formatNumber(share.limit) });
}

/**
 * A provider hold. Plan-limit and credit say what happened and do not promise
 * when it clears. An allowance hold waits for the next local day.
 */
export function holdLine(hold: Hold) {
  if (hold.reason === "allowance") return i18n.t("settings:usage.allowance.waiting");
  const name = providerShortName(hold.provider ?? undefined);
  if (hold.reason === "plan_limit") {
    return name ? i18n.t("settings:usage.allowance.planLimit", { name }) : i18n.t("settings:usage.allowance.planLimitUnnamed");
  }
  return name ? i18n.t("settings:usage.allowance.outOfCredit", { name }) : i18n.t("settings:usage.allowance.outOfCreditUnnamed");
}

/** One OpenRouter credit line. A missing reset stays off the sentence. */
export function creditLine(quota: OpenRouterQuota) {
  const remaining = money(quota.remainingUsd);
  const limit = money(quota.limitUsd);
  const when = quota.reset === "daily" ? i18n.t("settings:usage.allowance.reset.daily")
    : quota.reset === "weekly" ? i18n.t("settings:usage.allowance.reset.weekly")
      : quota.reset === "monthly" ? i18n.t("settings:usage.allowance.reset.monthly")
        : null;
  return when
    ? i18n.t("settings:usage.allowance.creditLeftReset", { remaining, limit, when })
    : i18n.t("settings:usage.allowance.creditLeft", { remaining, limit });
}

/** The short status is the whole line; a count names the work inside the sentence. */
export function allowanceNamesItself(state: Bucket["state"]) {
  return state === "ok" || state === "warning";
}
