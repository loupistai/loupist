import { ArrowLeftIcon, XIcon } from "lucide-react";
import { useTranslation } from "../../i18n";
import type { LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin } from "../../types";
import { Button } from "../ui/button";
import { SourceWorkspace } from "../knowledge/SourceWorkspace";
import { ResearchReader } from "../knowledge/ResearchReader";
export function MapReader({ bridge, catalog, location, onOpen, onPatch, onAsk, onChanged, onClose, onBack, onOpenLink, onManageProcessing }: {
  bridge: LoupistBridge; catalog: KnowledgeCatalog; location: KnowledgeLocation; onOpen: (location: KnowledgeLocation) => void; onPatch: (location: KnowledgeLocation) => void;
  onAsk: (pin: KnowledgePin, threadId?: string) => void; onChanged: () => void; onClose: () => void; onBack?: () => void; onOpenLink?: (url: string) => void | Promise<void>; onManageProcessing?: () => void;
}) {
  const { t } = useTranslation("knowledge");
  const shared = { bridge, catalog, location, onOpen, onPatch, onAsk, onChanged, onOpenLink };
  return <aside className="map-inspector map-reader" data-testid="map-reader" aria-label={t("map.reader")}>
    <div className="map-reader-actions" data-testid="map-reader-actions">{onBack ? <Button variant="ghost" size="sm" onClick={onBack}><ArrowLeftIcon />{t("map.back")}</Button> : <span />}<Button variant="ghost" size="icon-sm" aria-label={t("map.close")} onClick={onClose}><XIcon /></Button></div>
    {location.surface === "source" ? <SourceWorkspace {...shared} readerOnly onManageProcessing={onManageProcessing} /> : <ResearchReader {...shared} />}
  </aside>;
}
