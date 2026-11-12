import { useEffect, useState } from "react";
import { XIcon } from "lucide-react";
import type { LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin } from "../../types";
import { useTranslation } from "../../i18n";
import { Button } from "../ui/button";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "../ui/tabs";
import { SourceWorkspace } from "../knowledge/SourceWorkspace";
import { librarySurface, libraryState, useLibraryRequests, type LibraryTab } from "./libraryState";
import { ResearchLibrary } from "../knowledge/ResearchLibrary";

/** The note tab that shows what a location selects: a Journal entry opens Journal, a research note opens Notes. */
const noteTab = (location: KnowledgeLocation, fallback: LibraryTab): LibraryTab =>
  location.selection?.kind === "journal" ? "journal" : location.selection?.kind === "research" ? "note" : fallback === "source" ? "journal" : fallback;

export function MapLibrary({ bridge, catalog, location, onOpen, onAsk, onChanged, onClose, onAddSource, onManageProcessing }: {
  bridge: LoupistBridge; catalog: KnowledgeCatalog; location: KnowledgeLocation; onOpen: (location: KnowledgeLocation) => void;
  onAsk: (pin: KnowledgePin, threadId?: string) => void; onChanged: () => void; onClose: () => void; onAddSource?: () => void; onManageProcessing?: () => void;
}) {
  const { t } = useTranslation("knowledge");
  const savedState = libraryState(bridge, location.loupistId);
  const [tab, setTab] = useState<LibraryTab>(() => location.surface === "note" ? noteTab(location, savedState.tab) : savedState.tab);
  const [views, setViews] = useState(savedState.views);
  useEffect(() => { Object.assign(libraryState(bridge, location.loupistId), { tab, views }); }, [bridge, location.loupistId, tab, views]);
  useLibraryRequests(bridge, location.loupistId, state => { setTab(state.tab); setViews(state.views); });
  const surface = librarySurface(tab);
  const saved = views[tab] ?? { loupistId: location.loupistId, surface };
  const current = { ...saved, recordId: location.surface === surface ? location.recordId : undefined, revisionId: location.surface === surface ? location.revisionId : undefined, selection: location.surface === surface ? location.selection : undefined };
  const patch = (next: KnowledgeLocation) => setViews(v => ({ ...v, [tab]: next }));
  const shared = { bridge, catalog, location: current, query: current.view?.search ?? "", onQueryChange: (search: string) => patch({ ...current, view: { ...current.view, search } }), onPatch: patch, onOpen, onAsk, onChanged };
  return <aside className="map-library" data-testid="map-library" aria-label={t("map.library")}>
    <header><h2>{t("map.library")}</h2><Button variant="ghost" size="icon-sm" aria-label={t("map.closeLibrary")} onClick={onClose}><XIcon /></Button></header>
    <Tabs value={tab} onValueChange={value => setTab(value as typeof tab)}>
      <div className="map-library-tabbar"><TabsList aria-label={t("map.library")}><TabsTrigger value="source">{t("map.sources")}</TabsTrigger><TabsTrigger value="journal">{t("map.libraryJournal")}</TabsTrigger><TabsTrigger value="note">{t("map.libraryNotes")}</TabsTrigger></TabsList></div>
      <TabsContent value="source"><SourceWorkspace {...shared} compact onAddSource={onAddSource} onManageProcessing={onManageProcessing} /></TabsContent>
      <TabsContent value="journal"><ResearchLibrary {...shared} compact show="journal" /></TabsContent>
      <TabsContent value="note"><ResearchLibrary {...shared} compact show="notes" /></TabsContent>
    </Tabs>
  </aside>;
}
