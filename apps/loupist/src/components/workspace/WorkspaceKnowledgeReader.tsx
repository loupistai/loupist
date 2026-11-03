import i18n, { useTranslation } from "@/i18n";
import { useEffect, useRef, useState } from "react";
import type { LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin } from "../../types";
import { errorMessage } from "../../lib/errors";
import { LensWorkspace } from "../knowledge/LensWorkspace";
import { SourceWorkspace } from "../knowledge/SourceWorkspace";
import { ResearchReader } from "../knowledge/ResearchReader";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";

const t = i18n.getFixedT(null, "chat");

type Props = {
  bridge: LoupistBridge;
  location: KnowledgeLocation;
  onOpen: (location: KnowledgeLocation) => void;
  onPatch: (location: KnowledgeLocation) => void;
  onOpenPage: (location: KnowledgeLocation) => void;
  onAsk: (pin: KnowledgePin, threadId?: string) => void;
  onOpenLink?: (url: string) => void | Promise<void>;
};

/** The same exact readers used by World and Lens, without their library navigation. */
export function WorkspaceKnowledgeReader({ bridge, location, onOpen, onPatch, onOpenPage, onAsk, onOpenLink }: Props) {
  useTranslation("chat");
  const [catalog, setCatalog] = useState<KnowledgeCatalog | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const researchPane = useRef<HTMLDivElement>(null);
  const needsCatalog = location.surface === "source" || location.surface === "lens";
  useEffect(() => {
    if (!needsCatalog) return;
    let current = true;
    setError(null);
    void bridge.knowledgeCatalog({ loupistId: location.loupistId }).then((next) => {
      if (current) setCatalog(next);
    }).catch((cause: unknown) => { if (current) setError(errorMessage(cause, t("workspaceKnowledgeReader.couldNotOpenThisSavedReference"))); });
    return () => { current = false; };
  }, [bridge, location.loupistId, needsCatalog, reload]);
  const changed = () => setReload((value) => value + 1);
  if (location.surface === "note") return <div ref={researchPane} className="workspace-knowledge-reader workspace-research-reader" onScroll={(event) => onPatch({ ...location, view: { ...location.view, readerScrollTop: event.currentTarget.scrollTop } })}><ResearchReader bridge={bridge} location={location} onOpen={onOpen} onAsk={onAsk} onOpenLink={onOpenLink} onReady={() => { if (researchPane.current) researchPane.current.scrollTop = location.view?.readerScrollTop ?? 0; }} /></div>;
  if (location.surface === "world") return <div className="artifact-panel-state"><p>{t("workspaceKnowledgeReader.exploreThisSavedWorldAndItsEvidence")}</p><Button onClick={() => onOpenPage(location)}>{t("workspaceKnowledgeReader.openWorld")}</Button></div>;
  if (error) return <div className="artifact-panel-state" role="alert"><p>{error}</p><Button onClick={changed}>{t("workspaceKnowledgeReader.retryReference")}</Button></div>;
  if (!catalog || catalog.loupistId !== location.loupistId) return <p className="memory-loading-inline" role="status"><Spinner />{t("workspaceKnowledgeReader.openingSavedReference")}</p>;
  const props = { bridge, catalog, location, onOpen, onPatch, onAsk, onOpenLink, onChanged: changed, readerOnly: true };
  return <div className="workspace-knowledge-reader">{location.surface === "lens" ? <LensWorkspace {...props} /> : location.surface === "source" ? <SourceWorkspace {...props} /> : <p>{t("workspaceKnowledgeReader.thisReferenceIsUnavailable")}</p>}</div>;
}
