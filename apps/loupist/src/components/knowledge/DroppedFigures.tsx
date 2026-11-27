import { useEffect, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import type { GraphJobView, LoupistBridge } from "../../types";
import { Disclosure } from "../ui/disclosure";

/** Reads that run at once; rows fill as their jobs arrive (DESIGN 0). */
const CONCURRENT = 4;
/** A finished job's view does not change: one read per job for the window's life. */
const finished = new WeakMap<LoupistBridge, Map<string, Promise<GraphJobView | null>>>();
let running = 0;
const waiting: (() => void)[] = [];
const slot = () => new Promise<void>((resolve) => {
  const start = () => { running += 1; resolve(); };
  if (running < CONCURRENT) start(); else waiting.push(start);
});
const release = () => { running -= 1; waiting.shift()?.(); };

function finishedJob(bridge: LoupistBridge, loupistId: string, jobId: string) {
  const jobs = finished.get(bridge) ?? new Map<string, Promise<GraphJobView | null>>();
  finished.set(bridge, jobs);
  const key = `${loupistId}:${jobId}`;
  const known = jobs.get(key);
  if (known) return known;
  const read = slot().then(() => bridge.knowledgeGraphJob({ loupistId, jobId })).then(
    (job) => { release(); return job; },
    () => { release(); jobs.delete(key); return null; },
  );
  jobs.set(key, read);
  return read;
}

/** The facts a finished World job left out because their figures did not match the passage. */
export function useDroppedFigures(bridge: LoupistBridge, loupistId: string, jobId: string | null | undefined, finishedJob_: boolean) {
  const [job, setJob] = useState<GraphJobView | null>(null);
  useEffect(() => {
    setJob(null);
    if (!jobId || !finishedJob_) return undefined;
    let live = true;
    void finishedJob(bridge, loupistId, jobId).then((next) => { if (live) setJob(next); });
    return () => { live = false; };
  }, [bridge, loupistId, jobId, finishedJob_]);
  return job?.dropped?.figureMismatch ? job : null;
}

/**
 * "{n} facts were left out: their figures did not match the passage", with the examples
 * the host kept (at most eight) behind a disclosure (plan 4.1).
 */
export function DroppedFigures({ job }: { job: GraphJobView | null }) {
  const { t } = useTranslation("knowledge");
  const count = job?.dropped?.figureMismatch ?? 0;
  if (!job || !count) return null;
  const examples = job.droppedFigures ?? [];
  const line = t("sourceWorkspace.droppedFigures.line", { count, formattedCount: formatNumber(count) });
  if (!examples.length) return <p className="source-dropped-figures" data-dropped-figures={count}>{line}</p>;
  return <Disclosure className="source-dropped-figures" data-dropped-figures={count} title={line}>
    <ul>{examples.map((example, index) => <li key={index}>
      <span>{example.proposition}</span>
      <small>{t("sourceWorkspace.droppedFigures.stated", { figure: example.stated })}</small>
    </li>)}</ul>
  </Disclosure>;
}
