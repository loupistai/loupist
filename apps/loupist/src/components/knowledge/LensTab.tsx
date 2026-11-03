import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "../../i18n";
import type { KnowledgeCatalog, KnowledgeLocation, KnowledgePin, LoupistBridge } from "../../types";
import { HOST_STANDING_TITLE } from "../map/lensReading";
import { Button } from "../ui/button";
import { EmptyState } from "../ui/empty-state";
import { LensWorkspace } from "./LensWorkspace";
import { SourceWorkspace } from "./SourceWorkspace";
import { LensLearningPanel } from "./LensLearningPanel";

type Props = {
  bridge: LoupistBridge;
  location: KnowledgeLocation;
  refreshKey: number;
  onOpen: (next: KnowledgeLocation) => void;
  onPatch: (next: KnowledgeLocation) => void;
  onBack: () => void;
  onAsk: (pin: KnowledgePin, threadId?: string) => void;
  onOpenLink: (url: string) => void | Promise<void>;
  onChanged: () => void;
};

export function LensTab({ bridge, location, refreshKey, onOpen, onPatch, onBack, onAsk, onOpenLink, onChanged }: Props) {
  const { t, i18n } = useTranslation("knowledge");
  const { loupistId } = location;
  const [catalog, setCatalog] = useState<KnowledgeCatalog>();
  const [error, setError] = useState(false);
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let current = true;
    setError(false);
    void bridge.knowledgeCatalog({ loupistId }).then(result => {
      if (current) setCatalog(result);
    }).catch(() => { if (current) setError(true); });
    return () => { current = false; };
  }, [bridge, loupistId, refreshKey, retry]);

  const documents = useMemo(() => (catalog?.lenses ?? []).filter(lens => !lens.archived).map(lens => ({
    ...lens,
    displayTitle: lens.lensKind === "perspective" && lens.title === HOST_STANDING_TITLE ? t("lensWorkspace.defaultPerspective") : lens.title,
  })).sort((a, b) => Number(b.lensKind === "perspective") - Number(a.lensKind === "perspective")
    || a.displayTitle.localeCompare(b.displayTitle, i18n.language)), [catalog, t, i18n.language]);

  if (error) return <div role="alert">{t("map.catalogError")} <Button onClick={() => setRetry(n => n + 1)}>{t("map.retry")}</Button></div>;
  if (!catalog) return <p role="status">{t("lensWorkspace.openingLens")}</p>;
  const selected = location.surface === "lens" && location.recordId ? location : { loupistId, surface: "lens" as const, recordId: documents[0]?.recordId };
  const record = catalog.lenses.find(lens => lens.recordId === selected.recordId);
  const shared = { bridge, catalog, onAsk, onOpen, onOpenLink, onPatch, onChanged };
  return <div className="lens-tab">
    <nav className="lens-index" aria-label={t("lensWorkspace.documents")}>
      <h1>{t("lensWorkspace.documents")}</h1>
      <ul>{documents.map(lens => <li key={lens.recordId}>
        <button type="button" data-lens-document={lens.recordId} aria-current={selected.recordId === lens.recordId && location.surface !== "source" ? "page" : undefined}
          onClick={() => onOpen({ loupistId, surface: "lens", recordId: lens.recordId })}>
          <small>{t(`map.kind.${lens.lensKind ?? "method"}`)}</small><span>{lens.displayTitle}</span>
        </button>
      </li>)}</ul>
    </nav>
    {location.surface === "source" ? <section className="lens-source-reader">
      <Button size="sm" variant="ghost" onClick={onBack}>{t("memoryPage.backToMethod")}</Button>
      <SourceWorkspace {...shared} location={location} readerOnly />
    </section> : selected.recordId ? <LensWorkspace {...shared} key={selected.recordId} location={selected} markdownFirst>
      {record?.learned && !record.archived && record.lensKind === "method" ? <LensLearningPanel bridge={bridge} loupistId={loupistId}
        node={{ kind: "lens_method", recordId: record.recordId }}
        onSelect={node => onOpen({ loupistId, surface: "world", view: { followCurrent: true }, selection: { kind: "node", id: node.id } })}
        onArchived={() => { onPatch({ loupistId, surface: "lens" }); onChanged(); }} /> : null}
    </LensWorkspace> : <EmptyState title={t("lensWorkspace.noLensSelected")} description={t("lensWorkspace.chooseAPerspectiveOrMethodAskYourLoupistInChat")} />}
  </div>;
}
