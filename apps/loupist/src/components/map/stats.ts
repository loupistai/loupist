import type { TFunction } from "i18next";
import { formatNumber } from "../../i18n/format";

/** "6 names · 1 relationship": each count takes its own plural form. */
export function mapStatsText(t: TFunction<"knowledge">, names: number, links: number) {
  return t("map.stats", {
    names: t("map.statsNames", { count: names, formattedCount: formatNumber(names) }),
    links: t("map.statsLinks", { count: links, formattedCount: formatNumber(links) }),
  });
}
