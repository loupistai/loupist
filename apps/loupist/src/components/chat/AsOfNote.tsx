import { formatNumber } from "@/i18n/format";
import "@/styles/answer-checks.css";
import i18n, { useTranslation } from "@/i18n";
import type { AsOfReport } from "../../types";
import { formatAsOf } from "../../chat/asOf";
import { readNoun } from "../../chat/progressLabels";

const t = i18n.getFixedT(null, "chat");
/** Each list names this many items, then "+n more". */
const SHOWN = 4;

function names(items: string[]) {
  const unique = [...new Set(items)];
  const shown = unique.slice(0, SHOWN).join(", ");
  return unique.length > SHOWN ? `${shown} ${t("asOf.more", { formattedCount: formatNumber(unique.length - SHOWN) })}` : shown;
}

const LEFT_OUT_KEYS = ["facts", "sources", "notes", "methods"] as const;

/**
 * Under an as-of answer, what the cutoff kept out (finance-point-in-time.md
 * §2.3): reads that cannot show the past, reads with nothing dated by then,
 * and Memory the Loupist learned later. Bullets with nothing to say are left
 * out, and so is the note when no bullet remains.
 */
export function AsOfNote({ report, loupistName }: { report: AsOfReport; loupistName: string }) {
  useTranslation("chat");
  const date = formatAsOf(report.cutoff);
  const bullets: string[] = [];
  // Sources are named as nouns ("market data"), never as the step's action; the header already gives the date.
  if (report.refused?.length) bullets.push(t("asOf.refused", { names: names(report.refused.map((item) => readNoun(item.tool, { connection: item.connection }))) }));
  if (report.empty?.length) bullets.push(t("asOf.empty", { names: names(report.empty.map((item) => readNoun(item.tool, { target: item.target }))) }));
  const leftOut = LEFT_OUT_KEYS.flatMap((key) => {
    const count = report.left_out[key];
    return count > 0 ? [t(`asOf.${key}`, { count, formattedCount: formatNumber(count) })] : [];
  });
  if (leftOut.length) bullets.push(t("asOf.leftOut", { loupist: loupistName, items: leftOut.join(", ") }));
  if (!bullets.length) return null;
  const title = t("asOf.noteTitle", { date });
  return <section className="as-of-note" role="region" aria-label={title} aria-live="polite">
    <p>{title}</p>
    <ul>{bullets.map((bullet) => <li key={bullet}>{bullet}</li>)}</ul>
  </section>;
}
