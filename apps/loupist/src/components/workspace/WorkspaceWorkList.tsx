import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import { useEffect, useMemo, useRef, useState } from "react";
import { focusForKeyboard } from "../../lib/inputModality";
import { ChartNoAxesCombinedIcon, FileTextIcon, TableIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { isEmptyChatThreadId } from "../../app/emptyChat";
import type { ChatMessage, ChatArtifact, LoupistBridge } from "../../types";

const t = i18n.getFixedT(null, "chat");
/** The list shows this many rows, then "Show older" for the next page. */
export const WORK_PAGE = 50;

/** Files the host confirms these assistant turns wrote, at their latest write time. */
function writtenWorkspaceFiles(messages: readonly ChatMessage[]) {
  const writes = new Map<string, number>();
  for (const message of messages) {
    if (message.role !== "assistant") continue;
    for (const item of message.progress?.items ?? []) {
      if (item.kind !== "tool") continue;
      const time = item.finishedAtMs ?? item.startedAtMs;
      const remember = (path: string) => writes.set(path, Math.max(time, writes.get(path) ?? time));
      if (item.toolName === "run_python") {
        // A timed-out run can still write files. A retained filePath alone does not record a write.
        for (const path of item.writtenPaths ?? []) remember(path);
      } else if (item.status === "succeeded" && (item.operation === "write" || item.operation === "edit") && item.filePath) {
        remember(item.filePath);
      }
    }
  }
  return writes;
}

/** A chart the visible chat is still building; it has no saved artifact yet. */
export type BuildingWork = { key: string; title: string };

type Load = { status: "loading" | "ready" | "failed"; items: ChatArtifact[] };

/**
 * The panel's Work tab without an item open (side-panel.md P6): every chart and
 * document and written file from this chat, newest first, with the charts the
 * visible chat is building on top. Opening a row shows it in place.
 */
export function WorkspaceWorkList({ bridge, loupistId, threadId, messages, onOpenFile, refreshKey, building, returnTo, onReturned, onOpen }: {
  bridge: LoupistBridge;
  loupistId: string;
  threadId: string;
  messages?: readonly ChatMessage[];
  onOpenFile?: (path: string) => void;
  /** Changes when the visible chat publishes something, so the list re-reads. */
  refreshKey: string | number;
  building: readonly BuildingWork[];
  /** The item just closed with "All work": its row takes keyboard focus once the list is back. */
  returnTo?: string | null;
  onReturned?: () => void;
  onOpen: (item: ChatArtifact) => Promise<void>;
}) {
  useTranslation("chat");
  const [limit, setLimit] = useState(WORK_PAGE);
  const [load, setLoad] = useState<Load>({ status: "loading", items: [] });
  const [attempt, setAttempt] = useState(0);
  const [opening, setOpening] = useState<string | null>(null);
  const [openError, setOpenError] = useState(false);
  const listRef = useRef<HTMLUListElement>(null);
  const open = async (item: ChatArtifact) => {
    setOpening(item.id);
    setOpenError(false);
    try { await onOpen(item); }
    catch { setOpenError(true); }
    finally { setOpening(null); }
  };
  useEffect(() => {
    let live = true;
    setLoad((current) => ({ status: "loading", items: current.items }));
    void (isEmptyChatThreadId(threadId) ? Promise.resolve([]) : bridge.chatArtifactList(loupistId, threadId))
      .then((items) => { if (live) setLoad({ status: "ready", items }); })
      .catch(() => { if (live) setLoad((current) => ({ status: "failed", items: current.items })); });
    return () => { live = false; };
  }, [bridge, loupistId, threadId, refreshKey, attempt]);
  useEffect(() => {
    if (!returnTo || load.status === "loading") return;
    focusForKeyboard([...listRef.current?.querySelectorAll<HTMLElement>("[data-work-id]") ?? []].find((row) => row.dataset.workId === returnTo));
    onReturned?.();
  }, [load.status, returnTo, onReturned]);

  const files = useMemo(() => {
    const byPath = writtenWorkspaceFiles(messages ?? []);
    return [...byPath].map(([path, updated]) => ({ kind: "file" as const, path, updated }));
  }, [messages]);
  const rows = [...files, ...load.items.map(artifact => ({ kind: "artifact" as const, artifact, updated: artifact.updated_at_ms }))]
    .sort((a, b) => b.updated - a.updated);
  const loadError = load.status === "failed" ? <div className="artifact-panel-state" role="alert">
    <strong>{t("chatWorkspacePanel.couldNotLoadSavedWork")}</strong>
    <Button type="button" variant="outline" size="sm" onClick={() => setAttempt((value) => value + 1)}>{t("chatWorkspacePanel.tryAgain")}</Button>
  </div> : null;
  if (loadError && !rows.length && !building.length) return loadError;
  if (load.status === "loading" && !rows.length && !building.length) {
    return <div className="artifact-panel-state" role="status"><Spinner /><span>{t("workList.loading")}</span></div>;
  }
  if (!rows.length && !building.length) {
    return <div className="artifact-empty workspace-empty"><p>{t("workList.empty")}</p></div>;
  }
  const charts = load.items.filter((item) => item.kind === "chart").length;
  const tables = load.items.filter((item) => item.kind === "grid").length;
  const documents = load.items.length - charts - tables;
  return <div className="work-list">
    {loadError}
    {openError ? <p className="work-error" role="alert">{t("chatWorkspacePanel.couldNotOpenThisItem")}</p> : null}
    <p className="work-status">{[
      files.length ? t("workList.files", { count: files.length, formattedCount: formatNumber(files.length) }) : "",
      charts ? t("workList.charts", { count: charts, formattedCount: formatNumber(charts) }) : "",
      tables ? t("workList.tables", { count: tables, formattedCount: formatNumber(tables) }) : "",
      documents ? t("workList.documents", { count: documents, formattedCount: formatNumber(documents) }) : "",
    ].filter(Boolean).join(" · ")}</p>
    <ul ref={listRef} aria-label={t("workList.label")}>
      {building.map((item) => <li key={item.key}>
        <div className="work-row" data-state="building" role="status">
          <ChartNoAxesCombinedIcon aria-hidden="true" />
          <span className="work-row-text">
            <strong>{item.title || t("chatBlock.untitled")}</strong>
            <small><Spinner /> {t("chatBlock.building")}</small>
          </span>
        </div>
      </li>)}
      {rows.slice(0, limit).map((item) => {
        if (item.kind === "file") return <li key={`file:${item.path}`}>
          <button type="button" className="work-row" onClick={() => onOpenFile?.(item.path)}>
            <FileTextIcon aria-hidden="true" />
            <span className="work-row-text"><strong>{item.path.split("/").at(-1)}</strong><small>{item.path}</small></span>
          </button>
        </li>;
        const { artifact } = item;
        const facts = [
          artifact.kind === "chart" ? t("workspaceTabBar.chart") : artifact.kind === "grid" ? t("workspaceTabBar.table") : t("workspaceTabBar.document"),
          t("chatComposer.versionLabel", { version: artifact.current_revision }),
          formatDate(artifact.updated_at_ms, { year: undefined, month: "short", day: "numeric" }),
        ].join(" · ");
        return <li key={artifact.id}>
          <button type="button" className="work-row" data-work-id={artifact.id} aria-busy={opening === artifact.id || undefined} aria-disabled={opening !== null || undefined} onClick={() => { if (opening === null) void open(artifact); }}>
            {artifact.kind === "chart" ? <ChartNoAxesCombinedIcon aria-hidden="true" /> : artifact.kind === "grid" ? <TableIcon aria-hidden="true" /> : <FileTextIcon aria-hidden="true" />}
            <span className="work-row-text">
              <strong title={artifact.title}>{artifact.title}</strong>
              <small>{opening === artifact.id ? t("workspaceTabBar.opening") : facts}</small>
            </span>
          </button>
        </li>;
      })}
    </ul>
    {rows.length > limit ? <Button type="button" variant="ghost" size="sm" className="work-more" disabled={load.status === "loading"} onClick={() => setLimit((value) => value + WORK_PAGE)}>{t("workList.showOlder")}</Button> : null}
  </div>;
}
