import { useTranslation } from "@/i18n";
import { formatDate } from "@/i18n/format";
import type { LearnedMark as Mark } from "../../contracts/lens";
import { useGo } from "../navigation/Open";

/**
 * "Learned {date} · {program}" on a Lens section a learning run filed and nobody changed since.
 * It opens that run in Schedule; outside a navigator it is plain text, never a dead control.
 */
export function LearnedMark({ learned }: { learned?: Mark }) {
  const { t } = useTranslation("briefings");
  const go = useGo();
  if (!learned) return null;
  const label = t("programs.learned.chip", { date: formatDate(learned.atMs, { year: undefined, month: "short", day: "numeric" }), program: learned.programTitle });
  return go
    ? <button type="button" role="link" className="page-chip" data-chip="learned" title={label} aria-description={t("programs.learned.open")}
      onClick={() => go({ kind: "run", runId: learned.runId, taskId: learned.taskId })}><span className="page-chip-label">{label}</span></button>
    : <span className="page-chip" data-chip="learned" title={label}><span className="page-chip-label">{label}</span></span>;
}
