import i18n, { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { useState } from "react";
import { AtSignIcon, ChartLineIcon, FileIcon, FileTextIcon, QuoteIcon, WorkflowIcon, XIcon } from "lucide-react";
import type { ChatArtifactRef, KnowledgePin } from "../../types";
import type { ComposerChip } from "../../chat/composerChips";
import { AsOfChip } from "./ComposerAsOf";
import { knowledgePinKey } from "../../knowledge/location";
import type { InlineComposerItem } from "./useComposerInlineReferences";

const t = i18n.getFixedT(null, "chat");
/** At this many chips the tray shows the first few and folds the rest behind "+n more". */
const COLLAPSE_AT = 5;
const SHOWN_WHEN_COLLAPSED = 3;

type TrayItem = { key: string; kind: string; label: string; detail?: string; removeLabel: string; onRemove: () => void };

const pinKindLabel = (kind: KnowledgePin["kind"]) => ({
  source: t("chatComposer.source"), world_node: t("chatComposer.world_node"), world_claim: t("chatComposer.world_claim"),
  lens: t("chatComposer.lens"), journal: t("chatComposer.journal"), research: t("chatComposer.researchPin"),
})[kind];

/**
 * The marker before a chip's label (composer spec §2.4): map-style shapes for
 * World names, relationships and Lens entries, and small glyphs for documents,
 * files, quotes, charts and connections, so every kind reads the same way
 * wherever a chip was added from.
 */
function Marker({ kind }: { kind: string }) {
  if (kind === "world_node" || kind === "world_claim" || kind === "lens") return <i className="composer-chip-marker" data-kind={kind} aria-hidden="true" />;
  const Glyph = kind === "quote" ? QuoteIcon : kind === "artifact" ? ChartLineIcon : kind === "connection" ? AtSignIcon : kind === "skill" ? WorkflowIcon
    : kind === "file" ? FileIcon : FileTextIcon;
  return <Glyph aria-hidden="true" />;
}

/** One tray for everything attached to the next message: chips, pinned knowledge and selected work. */
export function ComposerChipTray({ chips, pins, artifact, asOf, onRemoveChip, onRemovePin, onClearArtifact, onEditAsOf, onRemoveAsOf }: {
  chips: readonly ComposerChip[];
  pins: readonly KnowledgePin[];
  artifact?: ChatArtifactRef;
  /** The next message's as-of date; its chip is always first (finance-point-in-time.md §2.2). */
  asOf?: string;
  onRemoveChip: (chip: ComposerChip) => void;
  onRemovePin?: (pin: KnowledgePin) => void;
  onClearArtifact?: () => void;
  onEditAsOf?: () => void;
  onRemoveAsOf?: () => void;
}) {
  useTranslation("chat");
  const [expanded, setExpanded] = useState(false);
  const items: TrayItem[] = [
    ...(artifact ? [{ key: `artifact:${artifact.artifact_id}`, kind: "artifact", label: artifact.title, detail: t("chatComposer.versionLabel", { version: artifact.revision }),
      removeLabel: t("chatComposer.removeSelectedWork"), onRemove: () => onClearArtifact?.() }] : []),
    ...pins.map((pin) => ({ key: `pin:${knowledgePinKey(pin)}`, kind: pin.kind === "source" && pin.spanId ? "source_span" : pin.kind, label: pin.label, detail: pinKindLabel(pin.kind),
      removeLabel: t("chatComposer.removeKnowledgePin", { label: pin.label }), onRemove: () => onRemovePin?.(pin) })),
    ...chips.map((chip) => ({ key: chip.key, kind: chip.kind, label: chip.label, removeLabel: t("chatComposer.removeContextChip", { label: chip.label }), onRemove: () => onRemoveChip(chip) })),
  ];
  if (!items.length && !asOf) return null;
  const collapsed = !expanded && items.length >= COLLAPSE_AT;
  const shown = collapsed ? items.slice(0, SHOWN_WHEN_COLLAPSED) : items;
  return <ul className="composer-chip-tray" aria-label={t("chatComposer.messageModes")}>
    {asOf ? <AsOfChip asOf={asOf} onEdit={onEditAsOf} onRemove={onRemoveAsOf} /> : null}
    {shown.map((item) => <li key={item.key} className="composer-chip" data-kind={item.kind} title={item.detail ? `${item.label} · ${item.detail}` : item.label}>
      <Marker kind={item.kind} />
      <span>{item.label}</span>
      {item.detail ? <small>{item.detail}</small> : null}
      <button type="button" aria-label={item.removeLabel} onClick={item.onRemove}><XIcon aria-hidden="true" /></button>
    </li>)}
    {collapsed ? <li><button type="button" className="composer-chip-more" aria-expanded={false} onClick={() => setExpanded(true)}>
      {t("chatComposer.moreChips", { formattedCount: formatNumber(items.length - SHOWN_WHEN_COLLAPSED) })}
    </button></li> : null}
  </ul>;
}

/** Native text editing removes inline references with Backspace/Delete; tab users also get named buttons. */
export function ComposerInlineReferenceControls({ items, onRemove }: { items: readonly InlineComposerItem[]; onRemove: (key: string) => void }) {
  useTranslation("chat");
  if (!items.length) return null;
  return <ul className="composer-inline-reference-controls sr-only" aria-label={t("chatComposer.addedToThisMessage")}>
    {items.map(item => <li key={item.key} data-kind={item.kind}><span>{item.label}</span><button type="button" aria-label={t("chatComposer.removeContextChip", { label: item.label })} onClick={() => onRemove(item.key)}><XIcon aria-hidden="true" /></button></li>)}
  </ul>;
}
