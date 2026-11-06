import { useEffect, useState } from "react";
import i18n, { useTranslation } from "@/i18n";
import type { LearnedDigest, LoupistBridge, PageView, SectionView } from "../../types";
import { learnedSummary } from "../../chat/receipt";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { useFixMemory } from "../memory/fixMemory";
import { pageTitle } from "../memory/PageReader";

const t = i18n.getFixedT(null, "chat");
const CHANGED = ["added", "replaced", "retired"] as const;
type Change = { opId: string; key: string; outcome: (typeof CHANGED)[number] };

const excerpt = (section: SectionView | undefined) => {
  const text = section?.markdown.replace(/\[([^\]]*)\]\([^)]*\)/gu, "$1").replace(/[#*_`>]/gu, "").replace(/\s+/gu, " ").trim() ?? "";
  return text.length > 140 ? `${text.slice(0, 140).trimEnd()}…` : text;
};

export function LearnedCard({ id, bridge, loupistId, digest, onOpenPage, onChanged }: {
  id: string;
  bridge?: LoupistBridge;
  loupistId: string;
  digest: LearnedDigest;
  onOpenPage: (recordId: string) => void;
  onChanged?: () => void;
}) {
  useTranslation("chat");
  const fix = useFixMemory();
  const [pages, setPages] = useState<ReadonlyMap<string, PageView>>(new Map());
  const [forgotten, setForgotten] = useState<ReadonlyMap<string, { opId: string; heading: string }>>(new Map());
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const summary = learnedSummary(digest);
  const recordIds = summary?.pages.map(page => page.recordId).join("\n") ?? "";

  useEffect(() => {
    if (!bridge || !recordIds) return;
    let live = true;
    void Promise.all(recordIds.split("\n").map(recordId => bridge.knowledgeLensRead({ loupistId, recordId }).catch(() => null)))
      .then(read => { if (live) setPages(new Map(read.flatMap(page => page ? [[page.recordId, page] as const] : []))); });
    return () => { live = false; };
  }, [bridge, loupistId, recordIds]);
  if (!summary) return null;

  const cell = (recordId: string, key: string) => `${recordId}\n${key}`;
  const current = (recordId: string, key: string) => {
    const body = pages.get(recordId)?.body;
    return body && body.kind !== "principle" ? body.sections.find(section => section.key === key) : undefined;
  };
  const apply = async (recordId: string, key: string, run: (bridge: LoupistBridge) => Promise<PageView>, after: (page: PageView) => void) => {
    if (!bridge || busy) return;
    setBusy(cell(recordId, key)); setError(null);
    try {
      const page = await run(bridge);
      setPages(existing => new Map(existing).set(recordId, page));
      after(page);
      onChanged?.();
    } catch (cause) { setError(errorMessage(cause, t("learnedCard.error"))); }
    finally { setBusy(null); }
  };
  const forget = (recordId: string, key: string, heading: string) => void apply(recordId, key, b => b.memorySectionForget({ loupistId, recordId, key }), page => {
    const entry = page.history.find(item => item.op === "retire" && item.key === key);
    if (entry) setForgotten(existing => new Map(existing).set(cell(recordId, key), { opId: entry.opId, heading }));
  });
  const undo = (recordId: string, key: string) => {
    const opId = forgotten.get(cell(recordId, key))?.opId;
    if (opId) void apply(recordId, key, b => b.memoryOpUndo({ loupistId, opId }), () => setForgotten(existing => { const next = new Map(existing); next.delete(cell(recordId, key)); return next; }));
  };

  return <div className="learned-card" id={id} role="group" aria-label={t("learnedCard.label")}>
    {summary.pages.map(page => {
      const view = pages.get(page.recordId);
      const changes: Change[] = page.outcomes.flatMap(outcome => CHANGED.includes(outcome.outcome as Change["outcome"]) && outcome.key ? [{ opId: outcome.opId, key: outcome.key, outcome: outcome.outcome as Change["outcome"] }] : []);
      return <section key={page.recordId} className="learned-page" data-learned-page={page.recordId}>
        <header>
          <button type="button" className="learned-page-title" onClick={() => onOpenPage(page.recordId)}>{pageTitle(view ?? page)}</button>
          <span className="learned-page-kind">{i18n.t(`knowledge:pageReader.kind.${page.kind}`)}{page.created ? ` · ${t("learnedCard.new")}` : ""}</span>
        </header>
        <ul>
          {changes.map(change => {
            const section = current(page.recordId, change.key);
            const gone = forgotten.has(cell(page.recordId, change.key));
            const heading = section?.heading ?? forgotten.get(cell(page.recordId, change.key))?.heading ?? change.key;
            const pending = busy === cell(page.recordId, change.key);
            return <li key={change.opId} data-outcome={change.outcome} data-forgotten={gone || undefined}>
              <div className="learned-section">
                <span className="learned-verb">{t(`learnedCard.outcome.${change.outcome}`)}</span>
                <strong>{heading}</strong>
                {section && !gone ? <p>{excerpt(section)}</p> : null}
                {gone ? <p role="status">{t("learnedCard.forgotten")}</p> : null}
              </div>
              <div className="learned-actions">
                {gone ? <Button variant="ghost" size="sm" disabled={pending} onClick={() => undo(page.recordId, change.key)}>{t("learnedCard.undo")}</Button> : section ? <>
                  {fix && view ? <Button variant="ghost" size="sm" aria-label={t("learnedCard.fixSection", { heading })} onClick={() => fix({ kind: "lens", loupistId, recordId: page.recordId, revisionId: view.revisionId, objectId: page.recordId, label: `${pageTitle(view)} · ${heading}` })}>{t("learnedCard.fix")}</Button> : null}
                  {bridge ? <Button variant="ghost" size="sm" disabled={pending} aria-label={t("learnedCard.forgetSection", { heading })} onClick={() => forget(page.recordId, change.key, heading)}>{t("learnedCard.forget")}</Button> : null}
                </> : null}
              </div>
            </li>;
          })}
        </ul>
      </section>;
    })}
    {error ? <p role="alert" className="learned-error">{error}</p> : null}
  </div>;
}
