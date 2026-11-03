import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import { useEffect, useState } from "react";
import { ChartNoAxesCombinedIcon, FileTextIcon, TableIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import type { LoupistArtifactItem, LoupistBridge } from "../../types";

const t = i18n.getFixedT(null, "chat");
/** The list shows this many rows, then "Show older" for the next page. */
export const WORK_PAGE = 50;

/** A chart the visible chat is still building; it has no saved artifact yet. */
export type BuildingWork = { key: string; title: string };

type Load = { status: "loading" | "ready" | "failed"; items: LoupistArtifactItem[] };

/**
 * The panel's Work tab without an item open (side-panel.md P6): every chart and
 * document this Loupist made, across its chats, newest first, with the charts the
 * visible chat is building on top. Opening a row shows it in place.
 */
export function WorkspaceWorkList({ bridge, loupistId, refreshKey, building, onOpen }: {
  bridge: LoupistBridge;
  loupistId: string;
  /** Changes when the visible chat publishes something, so the list re-reads. */
  refreshKey: string | number;
  building: readonly BuildingWork[];
  onOpen: (item: LoupistArtifactItem) => Promise<void>;
}) {
  useTranslation("chat");
  const [limit, setLimit] = useState(WORK_PAGE);
  const [load, setLoad] = useState<Load>({ status: "loading", items: [] });
  const [attempt, setAttempt] = useState(0);
  const [opening, setOpening] = useState<string | null>(null);
  const [openError, setOpenError] = useState(false);
  const open = async (item: LoupistArtifactItem) => {
    setOpening(item.artifact.id);
    setOpenError(false);
    try { await onOpen(item); }
    catch { setOpenError(true); }
    finally { setOpening(null); }
  };
  useEffect(() => {
    let live = true;
    setLoad((current) => ({ status: "loading", items: current.items }));
    void bridge.loupistArtifactList({ loupist_id: loupistId, limit })
      .then((items) => { if (live) setLoad({ status: "ready", items }); })
      .catch(() => { if (live) setLoad((current) => ({ status: "failed", items: current.items })); });
    return () => { live = false; };
  }, [bridge, loupistId, limit, refreshKey, attempt]);

  if (load.status === "failed" && !load.items.length) {
    return <div className="artifact-panel-state" role="alert">
      <strong>{t("chatWorkspacePanel.couldNotLoadSavedWork")}</strong>
      <Button type="button" variant="outline" size="sm" onClick={() => setAttempt((value) => value + 1)}>{t("chatWorkspacePanel.tryAgain")}</Button>
    </div>;
  }
  if (load.status === "loading" && !load.items.length && !building.length) {
    return <div className="artifact-panel-state" role="status"><Spinner /><span>{t("workList.loading")}</span></div>;
  }
  if (!load.items.length && !building.length) {
    return <div className="artifact-empty workspace-empty"><p>{t("workList.empty")}</p></div>;
  }
  const charts = load.items.filter((item) => item.artifact.kind === "chart").length;
  const tables = load.items.filter((item) => item.artifact.kind === "grid").length;
  const documents = load.items.length - charts - tables;
  return <div className="work-list">
    {openError ? <p className="work-error" role="alert">{t("chatWorkspacePanel.couldNotOpenThisItem")}</p> : null}
    <p className="work-status">{[
      charts ? t("workList.charts", { count: charts, formattedCount: formatNumber(charts) }) : "",
      tables ? t("workList.tables", { count: tables, formattedCount: formatNumber(tables) }) : "",
      documents ? t("workList.documents", { count: documents, formattedCount: formatNumber(documents) }) : "",
    ].filter(Boolean).join(" · ")}</p>
    <ul aria-label={t("workList.label")}>
      {building.map((item) => <li key={item.key}>
        <div className="work-row" data-state="building" role="status">
          <ChartNoAxesCombinedIcon aria-hidden="true" />
          <span className="work-row-text">
            <strong>{item.title || t("chatBlock.untitled")}</strong>
            <small><Spinner /> {t("chatBlock.building")}</small>
          </span>
        </div>
      </li>)}
      {load.items.map((item) => {
        const { artifact } = item;
        const facts = [
          artifact.kind === "chart" ? t("workspaceTabBar.chart") : artifact.kind === "grid" ? t("workspaceTabBar.table") : t("workspaceTabBar.document"),
          t("chatComposer.versionLabel", { version: artifact.current_revision }),
          formatDate(artifact.updated_at_ms, { year: undefined, month: "short", day: "numeric" }),
        ].join(" · ");
        const meta = item.chat_title ? `${facts} · ${item.chat_title}` : facts;
        return <li key={artifact.id}>
          <button type="button" className="work-row" aria-busy={opening === artifact.id || undefined} disabled={opening !== null} onClick={() => void open(item)}>
            {artifact.kind === "chart" ? <ChartNoAxesCombinedIcon aria-hidden="true" /> : artifact.kind === "grid" ? <TableIcon aria-hidden="true" /> : <FileTextIcon aria-hidden="true" />}
            <span className="work-row-text">
              <strong title={artifact.title}>{artifact.title}</strong>
              {/* The chat title gives way first; kind, version and date stay whole. */}
              <small title={meta}>{opening === artifact.id ? t("workspaceTabBar.opening") : <>
                <span className="work-row-facts">{facts}</span>
                {item.chat_title ? <span className="work-row-chat">· {item.chat_title}</span> : null}
              </>}</small>
            </span>
          </button>
        </li>;
      })}
    </ul>
    {load.items.length >= limit ? <Button type="button" variant="ghost" size="sm" className="work-more" disabled={load.status === "loading"} onClick={() => setLimit((value) => value + WORK_PAGE)}>{t("workList.showOlder")}</Button> : null}
  </div>;
}
