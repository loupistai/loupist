import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import i18n, { useTranslation } from "@/i18n";
import { useGo } from "../navigation/Open";
import { formatAsOf } from "../../chat/asOf";
import { lazy, memo, Suspense, useEffect, useState, type ReactNode } from "react";
import { EllipsisIcon, Maximize2Icon, Minimize2Icon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import type { ChatArtifactRef, ChatArtifactView } from "../../types";
import { sameBlock, type ChartBlock } from "../../chat/chartBlocks";
import { CHART_SCHEMA } from "../../contracts/chat";
import { chatArtifactRef } from "../artifacts/ArtifactContent";
import { datasetTsv } from "../artifacts/ArtifactData";
import { useChatExecution } from "./ChatExecutionContext";
import { ChartBoundary } from "../artifacts/ChartBoundary";
import { ChartSnapshotContext, type ChartSnapshotSlot } from "../charts/chartSnapshot";
import { useSaveChartPng } from "../charts/SaveChartPng";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";

const t = i18n.getFixedT(null, "chat");

const ArtifactChart = lazy(() => import("../artifacts/ArtifactChart"));

/** Expanding in place is offered only where a 480 px chart still leaves the thread readable. */
const WIDE_QUERY = "(min-width: 1440px)";

function useWideWindow() {
  const [wide, setWide] = useState(() => window.matchMedia?.(WIDE_QUERY).matches ?? false);
  useEffect(() => {
    const media = window.matchMedia?.(WIDE_QUERY);
    if (!media) return undefined;
    const update = () => setWide(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  return wide;
}

type Props = {
  block: ChartBlock;
  /** Opens the chart's latest revision in the panel. */
  onAsk?: (artifact: ChatArtifactRef) => void;
  askDisabled?: boolean;
  /** An as-of answer's date, shown after the title. */
  asOf?: string;
  /** A failed chart puts the question that asked for it back in the composer. */
  onAskAgain?: () => void;
};

type ViewState = { status: "loading" } | { status: "ready"; view: ChatArtifactView } | { status: "unavailable" };

/**
 * A chart in the answer itself. The card keeps one size from the moment the
 * Loupist starts building it until the chart is drawn, so the thread never jumps.
 * Each answer draws its retained revision and notes when a newer version exists.
 */
export const ChatArtifactBlock = memo(function ChatArtifactBlock({ block, onAsk, askDisabled, asOf, onAskAgain }: Props) {
  const go = useGo();
  useTranslation("chat");
  const scope = useChatExecution();
  const wide = useWideWindow();
  const [expanded, setExpanded] = useState(false);
  const [view, setView] = useState<ViewState>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);
  // The drawn chart leaves its snapshot here for "Save as PNG…".
  const [snapshot] = useState<ChartSnapshotSlot>(() => ({ current: null }));
  const artifactId = block.kind === "ready" ? block.artifact.artifact_id : null;
  const revision = block.kind === "ready" ? block.artifact.revision : null;
  const { state: copyState, copy } = useCopyFeedback(`${artifactId}:${revision}`, 1_500);
  const copied = copyState === "copied";
  const copyError = copyState === "failed";

  useEffect(() => {
    if (!artifactId) return undefined;
    if (!scope) { setView({ status: "unavailable" }); return undefined; }
    let live = true;
    setView({ status: "loading" });
    void scope.bridge.chatArtifactRead(scope.loupistId, scope.threadId, artifactId, revision ?? undefined)
      .then((next) => { if (live) setView({ status: "ready", view: next }); })
      .catch(() => { if (live) setView({ status: "unavailable" }); });
    return () => { live = false; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the scope's fields; a new scope object for the same thread must not re-read.
  }, [artifactId, revision, scope?.bridge, scope?.loupistId, scope?.threadId, attempt]);

  const title = block.kind === "ready" ? (view.status === "ready" ? view.view.artifact.title : block.artifact.title) : block.title;
  // The card owns the save, so a failure stays visible after the "…" menu closes.
  const png = useSaveChartPng(snapshot, title, (request) => scope ? scope.bridge.exportSave(request) : Promise.reject(new Error("no chat scope")));
  const building = block.kind === "building" || (block.kind === "ready" && block.updating);
  const current = view.status === "ready" && block.kind === "ready" ? view.view : null;
  // The answer recorded the revision it published; a newer stored one belongs to a later answer.
  const revisedLater = Boolean(current && block.kind === "ready" && !block.updating && current.artifact.current_revision > block.artifact.revision);
  const loaded = current && block.kind === "ready" && current.revision.revision === block.artifact.revision ? current : null;
  const dataset = loaded?.chart_dataset;
  const isExpanded = expanded && wide;
  // A card that was preparing while the reader watched fills with a fade; one read later is drawn at once.
  const [wasBuilding, setWasBuilding] = useState(building);
  if (building && !wasBuilding) setWasBuilding(true);
  const filled = wasBuilding && !building;

  const copyData = async () => {
    if (!dataset) return;
    await copy(datasetTsv(dataset));
  };

  let body: ReactNode = null;
  if (block.kind === "building") {
    // The first draw: the card already has its final size; still, ruled paper stands where the chart will appear.
    body = <div className="chat-artifact-block-ruled" aria-hidden="true"><i /><i /><i /><i /></div>;
  } else if (block.kind === "failed") {
    body = <div className="chat-artifact-block-state" role="alert">
      <p>{t("chatBlock.couldNotBuildChart")}</p>
      {onAskAgain ? <Button type="button" variant="ghost" size="sm" data-ask-again onClick={onAskAgain}>{t("chatBlock.askAgain")}</Button> : null}
    </div>;
  } else if (block.kind === "ready" && view.status === "unavailable") {
    body = <div className="chat-artifact-block-state" role="alert">
      <p>{t("chatBlock.unavailable")}</p>
      {scope ? <Button type="button" variant="outline" size="sm" onClick={() => setAttempt((value) => value + 1)}>{t("chatBlock.tryAgain")}</Button> : null}
    </div>;
  } else if (loaded && loaded.revision.payload.kind === "chart" && loaded.revision.payload.schema === CHART_SCHEMA && dataset && scope) {
    const payload = loaded.revision.payload;
    body = <ChartBoundary fallback={<p className="chat-artifact-block-state" role="alert">{t("chatBlock.unavailable")}</p>}><Suspense fallback={<p className="chat-artifact-block-state" role="status"><Spinner />{t("chatBlock.loading")}</p>}>
      <ChartSnapshotContext.Provider value={snapshot}>
        <ArtifactChart
          compact
          chart={payload.chart}
          dataset={dataset}
          theme={scope.theme}
        />
      </ChartSnapshotContext.Provider>
    </Suspense></ChartBoundary>;
  } else if (loaded) {
    body = <p className="chat-artifact-block-state" role="alert">{t("chatBlock.unavailable")}</p>;
  } else if (block.kind === "ready") {
    body = <p className="chat-artifact-block-state" role="status"><Spinner />{t("chatBlock.loading")}</p>;
  }

  return (
    <section
      className="chat-artifact-block"
      data-state={revisedLater ? "revised" : block.kind === "ready" ? "ready" : block.kind}
      data-expanded={isExpanded || undefined}
      data-filled={filled || undefined}
      aria-label={t("chatBlock.chartLabel", { title: title || t("chatBlock.untitled") })}
      aria-busy={building || undefined}
    >
      <header className="chat-artifact-block-header">
        <span className="chat-artifact-block-title" title={title}>{title || t("chatBlock.untitled")}</span>
        {asOf ? <span className="chat-artifact-block-asof">{t("asOf.chip", { date: formatAsOf(asOf) })}</span> : null}
        {building ? (
          <span className="chat-artifact-block-building" role="status">{t("chatBlock.building")}</span>
        ) : loaded ? (
          <>{/* Outside the bar and the menu, which take turns being hidden by width. */}
          {copyError ? <span className="chat-artifact-block-error" role="alert">{t("chatBlock.couldNotCopy")}</span>
            : png.failed ? <span className="chat-artifact-block-error" role="alert">{t("chartExport.couldNotSave")}</span> : null}
          <div className="chat-artifact-block-actions">
            {wide ? <Button type="button" variant="ghost" size="sm" aria-pressed={isExpanded} onClick={() => setExpanded((value) => !value)}>
              {isExpanded ? <Minimize2Icon aria-hidden="true" /> : <Maximize2Icon aria-hidden="true" />}
              {isExpanded ? t("chatBlock.collapse") : t("chatBlock.expand")}
            </Button> : null}
            {/* Text-only, like the table card's actions; under 720 px they fold into "…" (chat design change 25). */}
            {dataset ? <Button type="button" variant="ghost" size="sm" onClick={() => void copyData()}>
              {copied ? t("chatBlock.copied") : t("chatBlock.copyData")}
            </Button> : null}
            {dataset && scope ? <Button type="button" variant="ghost" size="sm" disabled={png.busy} onClick={() => void png.save()}>{t("chartExport.savePng")}</Button> : null}
            {onAsk ? <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={askDisabled}
              title={askDisabled ? t("artifactContent.waitForThisResponseToFinish") : undefined}
              aria-label={t("chatBlock.askAbout", { title })}
              onClick={() => onAsk(chatArtifactRef(loaded))}
            >
              {t("chatBlock.ask")}
            </Button> : null}
          </div>
          <ChartCardMore copy={dataset ? () => void copyData() : undefined} png={dataset && scope ? png : undefined}
            ask={onAsk ? () => onAsk(chatArtifactRef(loaded)) : undefined} askDisabled={askDisabled} /></>
        ) : null}
      </header>
      {revisedLater && current ? <div className="chat-artifact-block-header" role="status">
        <span className="chat-artifact-block-asof">{t("chatBlock.updatedSince")}</span>
        {go && scope ? <Button type="button" variant="ghost" size="sm" onClick={() => {
          void scope.bridge.chatArtifactRead(scope.loupistId, scope.threadId, current.artifact.id)
            .then((latest) => go({ kind: "artifact", artifact: chatArtifactRef(latest) }))
            .catch(() => setView({ status: "unavailable" }));
        }}>{t("chatBlock.openLatest")}</Button> : null}
      </div> : null}
      <div className="chat-artifact-block-body">{body}</div>
    </section>
  );
}, (before, after) => sameBlock(before.block, after.block) && before.onAsk === after.onAsk && before.askDisabled === after.askDisabled
  && before.asOf === after.asOf && before.onAskAgain === after.onAskAgain);

/** The chart card's actions folded into one "…" menu under 720 px, as the table card's are. */
function ChartCardMore({ copy, png, ask, askDisabled }: {
  copy?: () => void; png?: { save: () => Promise<void>; busy: boolean }; ask?: () => void; askDisabled?: boolean;
}) {
  useTranslation("chat");
  return <div className="chat-artifact-block-more">
    <DropdownMenu>
      <DropdownMenuTrigger asChild><Button type="button" variant="ghost" size="icon-sm" aria-label={t("grid.more")}><EllipsisIcon aria-hidden="true" /></Button></DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {copy ? <DropdownMenuItem onSelect={copy}>{t("chatBlock.copyData")}</DropdownMenuItem> : null}
        {png ? <DropdownMenuItem disabled={png.busy} onSelect={() => void png.save()}>{t("chartExport.savePng")}</DropdownMenuItem> : null}
        {ask ? <DropdownMenuItem disabled={askDisabled} onSelect={ask}>{t("chatBlock.ask")}</DropdownMenuItem> : null}
      </DropdownMenuContent>
    </DropdownMenu>
  </div>;
}

