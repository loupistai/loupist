import type { TFunction } from "i18next";
import { formatNumber } from "../../i18n/format";

const WEEK_MS = 7 * 86_400_000;

/** "6 names · 1 relationship": each count takes its own plural form. */
export function mapStatsText(t: TFunction<"knowledge">, names: number, links: number) {
  return t("map.stats", {
    names: t("map.statsNames", { count: names, formattedCount: formatNumber(names) }),
    links: t("map.statsLinks", { count: links, formattedCount: formatNumber(links) }),
  });
}

/** Projected entities first seen in the last seven days, never above the entity total. */
export function weeklyNameCount(entities: readonly { firstSeenAtMs?: number }[], nowMs: number, total: number) {
  const since = nowMs - WEEK_MS;
  const count = entities.filter(node => node.firstSeenAtMs != null && node.firstSeenAtMs >= since && node.firstSeenAtMs <= nowMs).length;
  return Math.min(count, Math.max(0, total));
}

export type WorldStatus = {
  shownNames: number;
  totalNames: number;
  bounded: boolean;
  links: number;
  disagreements: number;
  weekly: number;
};

/**
 * The toolbar counters, each with its unit, joined by " · ".
 * A bounded World says how many names are shown and leaves out the weekly part.
 */
export function worldStatusLine(t: TFunction<"knowledge">, input: WorldStatus) {
  const names = input.bounded
    ? t("map.bounded", { shown: formatNumber(input.shownNames), total: formatNumber(input.totalNames) })
    : t("map.statsNameCount", { count: input.shownNames, formattedCount: formatNumber(input.shownNames) });
  const parts = [names, t("map.statsLinks", { count: input.links, formattedCount: formatNumber(input.links) })];
  if (!input.bounded && input.weekly > 0) parts.push(t("map.weeklyNames", { count: input.weekly, formattedCount: formatNumber(input.weekly) }));
  if (input.disagreements > 0) parts.push(t("map.disagreements", { count: input.disagreements, formattedCount: formatNumber(input.disagreements) }));
  return parts.join(" · ");
}
