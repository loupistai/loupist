import i18n, { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { useState } from "react";
import { WorkflowIcon, XIcon } from "lucide-react";
import type { ComposerChip } from "../../chat/composerChips";
import { AsOfChip } from "./ComposerAsOf";
import type { InlineComposerItem } from "./useComposerInlineReferences";

const t = i18n.getFixedT(null, "chat");
/** At this many chips the tray shows the first few and folds the rest behind "+n more". */
const COLLAPSE_AT = 5;
const SHOWN_WHEN_COLLAPSED = 3;

type TrayItem = { key: string; kind: string; label: string; removeLabel: string; onRemove: () => void };

/** The next message’s Skill modes; object references stay inline in the textarea. */
export function ComposerChipTray({ chips, asOf, onRemoveChip, onEditAsOf, onRemoveAsOf }: {
  chips: readonly ComposerChip[];
  /** The next message's as-of date; its chip is always first (finance-point-in-time.md §2.2). */
  asOf?: string;
  onRemoveChip: (chip: ComposerChip) => void;
  onEditAsOf?: () => void;
  onRemoveAsOf?: () => void;
}) {
  useTranslation("chat");
  const [expanded, setExpanded] = useState(false);
  const items: TrayItem[] = [
    ...chips.map((chip) => ({ key: chip.key, kind: chip.kind, label: chip.label, removeLabel: t("chatComposer.removeContextChip", { label: chip.label }), onRemove: () => onRemoveChip(chip) })),
  ];
  if (!items.length && !asOf) return null;
  const collapsed = !expanded && items.length >= COLLAPSE_AT;
  const shown = collapsed ? items.slice(0, SHOWN_WHEN_COLLAPSED) : items;
  return <ul className="composer-chip-tray" aria-label={t("chatComposer.messageModes")}>
    {asOf ? <AsOfChip asOf={asOf} onEdit={onEditAsOf} onRemove={onRemoveAsOf} /> : null}
    {shown.map((item) => <li key={item.key} className="composer-chip" data-kind={item.kind} title={item.label}>
      <WorkflowIcon aria-hidden="true" />
      <span>{item.label}</span>
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
