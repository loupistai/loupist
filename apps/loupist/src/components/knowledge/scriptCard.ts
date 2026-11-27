import type { ScriptSummary } from "../../contracts/generated";
import type { ScheduledTaskListItem } from "../../contracts/scheduled";
import { humanizeToolName } from "../connections/custom/toolTitle";

const OUTCOMES = ["met", "not_met", "error", "no_data", "rate_limited", "stale", "skipped_busy"] as const;
export type ScriptOutcome = (typeof OUTCOMES)[number];

export function folded(value: string) {
  return value.normalize("NFKC").replace(/\s+/g, " ").trim().toLocaleLowerCase();
}

/** One line of a reading, at most 80 rendered characters. */
export function oneReading(value: string, limit = 80) {
  const line = value.split(/\r?\n/).map(part => part.trim()).find(Boolean) ?? "";
  const chars = [...line];
  if (chars.length <= limit) return line;
  return `${chars.slice(0, limit - 1).join("").trimEnd()}…`;
}

export function scriptOutcome(value: string): ScriptOutcome | undefined {
  return (OUTCOMES as readonly string[]).includes(value) ? value as ScriptOutcome : undefined;
}

export function scriptReads(script: ScriptSummary) {
  const labels = script.metadata.reads.map(read => humanizeToolName(read.tool)).filter(Boolean);
  return { labels: labels.slice(0, 3), extra: Math.max(0, labels.length - 3) };
}

/** The task whose check runs this script, when the list has one. */
export function scriptTask(script: ScriptSummary, tasks: readonly ScheduledTaskListItem[]) {
  return tasks.find(task => task.trigger?.script.name === script.name);
}
