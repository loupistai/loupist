import { useEffect, useState } from "react";
import { XIcon } from "lucide-react";
import type { LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin } from "../../types";
import { useTranslation } from "../../i18n";
import { Button } from "../ui/button";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "../ui/tabs";
import { SourceWorkspace } from "../knowledge/SourceWorkspace";
import { libraryState, type LibraryTab } from "./libraryState";
import { ResearchLibrary } from "../knowledge/ResearchLibrary";
export function MapLibrary({ bridge, catalog, location, onOpen, onAsk, onChanged, onClose, onAddSource, onManageProcessing }: {
  bridge: LoupistBridge; catalog: KnowledgeCatalog; location: KnowledgeLocation; onOpen: (location: KnowledgeLocation) => void;
  onAsk: (pin: KnowledgePin, threadId?: string) => void; onChanged: () => void; onClose: () => void; onAddSource?: () => void; onManageProcessing?: () => void;
}) {
  const { t } = useTranslation("knowledge");
  const savedState = libraryState(bridge, location.loupistId);
  const [tab, setTab] = useState<LibraryTab>(() => Object.keys(savedState.views).length ? savedState.tab : location.surface === "note" ? "note" : savedState.tab);
  const [views, setViews] = useState(savedState.views);
  useEffect(() => { Object.assign(libraryState(bridge, location.loupistId), { tab, views }); }, [bridge, location.loupistId, tab, views]);
  const saved = views[tab] ?? { loupistId: location.loupistId, surface: tab };
  const current = { ...saved, recordId: location.surface === tab ? location.recordId : undefined, revisionId: location.surface === tab ? location.revisionId : undefined, selection: location.surface === tab ? location.selection : undefined };
  const patch = (next: KnowledgeLocation) => setViews(v => ({ ...v, [tab]: next }));
  const shared = { bridge, catalog, location: current, query: current.view?.search ?? "", onQueryChange: (search: string) => patch({ ...current, view: { ...current.view, search } }), onPatch: patch, onOpen, onAsk, onChanged };
  return <aside className="map-library" aria-label={t("map.library")}>
    <header><h2>{t("map.library")}</h2><Button variant="ghost" size="icon-sm" aria-label={t("map.closeLibrary")} onClick={onClose}><XIcon /></Button></header>
    <Tabs value={tab} onValueChange={value => setTab(value as typeof tab)}>
      <div className="map-library-tabbar"><TabsList aria-label={t("map.library")}><TabsTrigger value="source">{t("map.sources")}</TabsTrigger><TabsTrigger value="note">{t("journal.notes")}</TabsTrigger></TabsList></div>
      <TabsContent value="source"><SourceWorkspace {...shared} compact onAddSource={onAddSource} onManageProcessing={onManageProcessing} /></TabsContent>
      <TabsContent value="note"><ResearchLibrary {...shared} compact /></TabsContent>
    </Tabs>
  </aside>;
}
