/**
 * A host run's history line in the reader's language. Rust records what the
 * results task did as structured items (`RunSummaryItem`, `commands/earnings.rs`);
 * this words each one, naming the asset as the app shows it.
 */
import i18n from "@/i18n";
import { formatNumber } from "@/i18n/format";
import type { RunSummaryItem } from "../../contracts/scheduled";

const t = i18n.getFixedT(null, "briefings");

export function runSummary(items: readonly RunSummaryItem[], nameOf: (name: string) => string = (name) => name) {
  return items.map((item) => {
    switch (item.kind) {
      case "earnings_nothing": return t("schedule.earningsRun.nothing");
      // The reason stays in the run's diagnostics; the line says which capture failed.
      case "earnings_failed": return t("schedule.earningsRun.failed", { name: nameOf(item.name), period: item.period });
      case "earnings_expected":
      case "earnings_reported":
        return t(item.kind === "earnings_expected" ? "schedule.earningsRun.expected" : "schedule.earningsRun.reported",
          { name: nameOf(item.name), period: item.period, count: item.count, formattedCount: formatNumber(item.count) });
    }
  }).join(" ");
}
