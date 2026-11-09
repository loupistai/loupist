import { formatDateTime } from "@/i18n/format";
import { ChartBoundary } from "./ChartBoundary";
import i18n, { useTranslation } from "@/i18n";
import { lazy, Suspense, useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { CheckIcon, ClipboardIcon, MessageCircleIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { SafeMessageResponse } from "@/components/chat/SafeMessageResponse";
import type { ChatArtifactRef, ChatArtifactView } from "../../types";
import { CHART_SCHEMA, GRID_SCHEMA } from "../../contracts/chat";
import { ArtifactData } from "./ArtifactData";
import { ConflictCard, GridCard } from "@/components/chat/GridCard";
import { ChartSnapshotContext, type ChartSnapshotSlot } from "../charts/chartSnapshot";
import { SaveChartPng } from "../charts/SaveChartPng";

const t = i18n.getFixedT(null, "chat");

const ArtifactChart = lazy(() => import("./ArtifactChart"));

type Props = {
  view: ChatArtifactView;
  theme: "light" | "dark";
  workspacePath?: string;
  onAsk?: (artifact: ChatArtifactRef) => void;
  askDisabled?: boolean;
  onExport?: (request: { suggestedName: string; bytes: number[]; kind: "csv" | "md" | "png" }) => Promise<string | null>;
};

export const chatArtifactRef = (view: ChatArtifactView): ChatArtifactRef => ({
  artifact_id: view.artifact.id,
  revision: view.revision.revision,
  kind: view.artifact.kind,
  title: view.artifact.title,
  digest: view.revision.digest,
});

/**
 * The Work status row's line for an open item (side-panel.md §2.2, §3.4): its title and kind, the version
 * control, and when it last changed. It shares the row with "All work", so a short dock keeps its height for the item.
 * A table's card names itself, so its line keeps the kind only.
 */
export function ArtifactStatus({ view, version }: { view: ChatArtifactView; version: ReactNode }) {
  useTranslation("chat");
  const kind = view.artifact.kind === "chart" ? t("workspaceTabBar.chart") : view.artifact.kind === "grid" ? t("workspaceTabBar.table") : t("workspaceTabBar.document");
  return <>
    <span className="workspace-status-text">{view.artifact.kind === "grid" ? kind : <><span>{view.artifact.title}</span>{" · "}{kind}</>}</span>
    {version}
    <span className="workspace-status-text workspace-status-updated">
      {/* The shown revision's own date, so an earlier version says when it was made. */}
      <time dateTime={new Date(view.revision.created_at_ms).toISOString()}>
        {t("artifactContent.updatedAt", { date: formatDateTime(view.revision.created_at_ms, { year: undefined, month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }) })}
      </time>
    </span>
  </>;
}

export function ArtifactContent({
  view,
  theme,
  workspacePath,
  onAsk,
  askDisabled,
  onExport,
}: Props) {
  useTranslation("chat");
  const [mode, setMode] = useState<"preview" | "source" | "data">("preview");
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState<string | null>(null);
  const viewId = useId();
  const copyRequest = useRef(0);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // The drawn chart leaves its snapshot here for "Save as PNG…".
  const [snapshot] = useState<ChartSnapshotSlot>(() => ({ current: null }));

  useEffect(() => {
    setMode("preview");
    setCopied(false);
    setCopyError(null);
    ++copyRequest.current;
    return () => {
      ++copyRequest.current;
      if (copyTimer.current !== null) clearTimeout(copyTimer.current);
    };
  }, [view.artifact.id, view.revision.revision]);

  const copySource = async () => {
    if (view.revision.payload.kind !== "markdown") return;
    const request = ++copyRequest.current;
    try {
      await navigator.clipboard.writeText(view.revision.payload.markdown);
      if (request !== copyRequest.current) return;
      setCopied(true);
      setCopyError(null);
      if (copyTimer.current !== null) clearTimeout(copyTimer.current);
      copyTimer.current = setTimeout(() => setCopied(false), 1_500);
    } catch {
      if (request === copyRequest.current) setCopyError(t("artifactContent.couldNotCopyThisDocumentTryAgain"));
    }
  };

  if (view.revision.payload.kind === "chart" && view.revision.payload.schema !== CHART_SCHEMA) {
    return <div className="artifact-panel-state" role="alert">{t("artifactChart.unsupportedFormat")}</div>;
  }
  if (view.revision.payload.kind === "grid") {
    // A grid opens as the same card the answer shows, full width (finance-grid-answers.md §2.3).
    const { grid, schema } = view.revision.payload;
    if (schema !== GRID_SCHEMA) return <div className="artifact-panel-state" role="alert">{t("artifactChart.unsupportedFormat")}</div>;
    const evidenceRefs = view.evidence_refs ?? [];
    return <div className="workspace-artifact-view workspace-grid-view">
      {grid.layout === "conflict"
        ? <ConflictCard title={view.artifact.title} conflict={grid} evidenceRefs={evidenceRefs} />
        : <GridCard title={view.artifact.title} grid={grid} evidenceRefs={evidenceRefs} onExport={onExport}
          onAsk={onAsk ? () => onAsk(chatArtifactRef(view)) : undefined} askDisabled={askDisabled} />}
    </div>;
  }

  const isMarkdown = view.artifact.kind === "markdown";
  const detailMode = isMarkdown ? "source" : "data";
  const changeViewWithKeyboard = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const next = event.key === "Home" ? "preview" : event.key === "End" ? detailMode
      : mode === "preview" ? detailMode : "preview";
    setMode(next);
    document.getElementById(`${viewId}-${next}`)?.focus();
  };

  return (
    <div className="workspace-artifact-view">
      <div className="artifact-view-toolbar">
        <div className="artifact-view-leading">
          <div className="artifact-view-tabs" role="tablist" aria-label={t("artifactContent.workView")}>
            <button
              type="button"
              role="tab"
              id={`${viewId}-preview`}
              aria-selected={mode === "preview"}
              aria-controls={`${viewId}-content`}
              tabIndex={mode === "preview" ? 0 : -1}
              onKeyDown={changeViewWithKeyboard}
              onClick={() => setMode("preview")}
            >
              {isMarkdown ? t("artifactContent.preview") : t("artifactContent.chart")}
            </button>
            <button
              type="button"
              role="tab"
              id={`${viewId}-${detailMode}`}
              aria-selected={mode === detailMode}
              aria-controls={`${viewId}-content`}
              tabIndex={mode === detailMode ? 0 : -1}
              onKeyDown={changeViewWithKeyboard}
              onClick={() => setMode(detailMode)}
            >
              {isMarkdown ? t("artifactContent.source") : t("artifactContent.data")}
            </button>
          </div>
        </div>
        <div className="artifact-toolbar-actions">
          {isMarkdown ? <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => void copySource()}
          >
            {copied ? <CheckIcon /> : <ClipboardIcon />}
            {copied ? t("artifactContent.copied") : t("artifactContent.copy")}
          </Button> : null}
          {!isMarkdown && mode === "preview" && view.chart_dataset && onExport ? <SaveChartPng slot={snapshot} title={view.artifact.title} onSave={onExport} /> : null}
          {onAsk ? <Button type="button" size="sm" variant="ghost" onClick={() => onAsk(chatArtifactRef(view))} disabled={askDisabled} title={askDisabled ? t("artifactContent.waitForThisResponseToFinish") : t("artifactContent.askAboutThisOrDescribeAChange")}><MessageCircleIcon />{t("artifactContent.askAboutThis")}</Button> : null}
        </div>
      </div>
      {copyError ? <p className="artifact-action-error" role="alert">{copyError}</p> : null}
      <div className="artifact-view-content" id={`${viewId}-content`} role="tabpanel" aria-labelledby={`${viewId}-${mode}`} tabIndex={0}>
        {mode === "data" && view.chart_dataset ? (
          <ArtifactData key={`${view.artifact.id}:${view.revision.revision}`} dataset={view.chart_dataset} title={view.artifact.title} onExport={onExport} />
        ) : mode === "source" && view.revision.payload.kind === "markdown" ? (
          <pre className="artifact-source">
            <code>{view.revision.payload.markdown}</code>
          </pre>
        ) : view.revision.payload.kind === "markdown" ? (
          <article className="artifact-markdown">
            <SafeMessageResponse
              text={view.revision.payload.markdown}
              isAnimating={false}
              workspacePath={workspacePath}
            />
          </article>
        ) : view.chart_dataset ? (
          <ChartBoundary fallback={<div className="artifact-panel-state" role="alert">{t("artifactContent.chartDataIsUnavailable")}</div>}><Suspense
            fallback={
              <div className="artifact-panel-state" role="status">
                <Spinner />
                <span>{t("artifactContent.loadingChart")}</span>
              </div>
            }
          >
            <ChartSnapshotContext.Provider value={snapshot}>
              <ArtifactChart
                onAskRange={onAsk && !askDisabled ? (range) => onAsk({ ...chatArtifactRef(view), selected_range: range }) : undefined}
                chart={view.revision.payload.chart}
                dataset={view.chart_dataset}
                theme={theme}
              />
            </ChartSnapshotContext.Provider>
          </Suspense></ChartBoundary>
        ) : (
          <div className="artifact-panel-state" role="alert">
            {t("artifactContent.chartDataIsUnavailable")}</div>
        )}
      </div>
    </div>
  );
}
