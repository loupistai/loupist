import { formatNumber } from "../../i18n/format";
import { useTranslation } from "../../i18n";
import { fromMarkdown } from "mdast-util-from-markdown";
import { ArrowDownLeftIcon, ArrowUpRightIcon, ListTreeIcon } from "lucide-react";
import type { LensView } from "../../types";
import { Button } from "../ui/button";

export type LensOutlineItem = { key: string; title: string; depth: number; position?: number; headingIndex?: number };

export function markdownOutline(markdown: string): LensOutlineItem[] {
  const tree = fromMarkdown(markdown);
  type Node = { type: string; value?: string; alt?: string | null; children?: Node[]; depth?: number; position?: { start: { offset?: number } } };
  const text = (node: Node): string => node.value ?? node.alt ?? (node.children ?? []).map(text).join("");
  const outline: LensOutlineItem[] = [];
  const visit = (node: Node) => {
    if (node.type === "heading") outline.push({ key: `heading-${outline.length}`, title: text(node), depth: node.depth ?? 1, position: node.position?.start.offset ?? 0, headingIndex: outline.length });
    else node.children?.forEach(visit);
  };
  visit(tree);
  return outline;
}

type Props = { outline: LensOutlineItem[]; view: LensView; onJump: (item: LensOutlineItem) => void; onOpen: (recordId: string) => void };

export function LensContextPane({ outline, view, onJump, onOpen }: Props) {
  const { t } = useTranslation("knowledge");
  const groups = [
    { title: t("lensContextPane.links"), label: t("lensContextPane.lensLinks"), icon: ArrowUpRightIcon, items: view.links ?? [], truncated: view.linksTruncated, empty: t("lensContextPane.noLinksToOtherLensesYet") },
    { title: t("lensContextPane.backlinks"), label: t("lensContextPane.lensBacklinks"), icon: ArrowDownLeftIcon, items: view.backlinks ?? [], truncated: view.backlinksTruncated, empty: t("lensContextPane.noOtherLensLinksHereYet") },
  ];
  return <div className="lens-context-content">
    <nav aria-label={t("lensContextPane.lensOutline")} className="lens-context-section"><h3><ListTreeIcon />{t("lensContextPane.outline")}</h3>{outline.length ? <ul>{outline.map((item) => <li key={item.key} style={{ paddingInlineStart: `${Math.min(item.depth - 1, 3) * 10}px` }}><Button type="button" variant="ghost" onClick={() => onJump(item)}>{item.title}</Button></li>)}</ul> : <p>{t("lensContextPane.noHeadingsInThisLens")}</p>}</nav>
    {groups.map(({ title, label, icon: Icon, items, truncated, empty }) => <section key={title} className="lens-context-section" aria-label={label}><h3><Icon />{title}<span>{truncated ? t("lensContextPane.truncatedCount", { countText: formatNumber(items.length) }) : formatNumber(items.length)}</span></h3>{items.length ? <ul>{items.map((item) => <li key={item.recordId}><Button type="button" variant="ghost" onClick={() => onOpen(item.recordId)}>{item.title}</Button></li>)}</ul> : <p>{empty}</p>}</section>)}
    {view.unresolvedLinks?.length ? <p className="lens-context-note">{t("lensContextPane.unavailableLinks", { count: view.unresolvedLinks.length, countText: formatNumber(view.unresolvedLinks.length) })}</p> : null}
    <p className="lens-context-note">{t("lensContextPane.backlinksShowTheCurrentCollection")}</p>
  </div>;
}
