import i18n, { useTranslation } from "@/i18n";
import { scheduledTaskTitle } from "../briefings/scheduledPresentation";
import { chatTitle } from "../../app/chatTitle";
import { formatDate } from "@/i18n/format";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import type { ChatThread, LoupistSummary, LoupistBridge } from "../../types";
import type { AttentionItem } from "../../contracts/attention";
import type { KnowledgeMapNode } from "../../contracts/knowledge";
import { nodeColorKey } from "../../knowledge/mapKinds";
import { entityLabel } from "../../knowledge/mapItems";
import { readKnowledgeMap } from "../../knowledge/mapRead";
import { attentionNamesLoupist, attentionWhat } from "./NeedsAttention";
import { assetExchangeLabel, type AssetRow } from "../../contracts/assets";
import { useAssetPageOpener } from "../assets/assetOpener";
import { useAssetNames } from "../assets/assetName";
import { tickerLabel } from "@/lib/market";

const t = i18n.getFixedT(null, "app");
/** Each group shows this many rows before "More from {group}". */
const ROWS_PER_GROUP = 4;
const RECENT_CHATS = 3;
/** Names, principles and briefings are read from at most this many Loupists, the active one first. */
export const SEARCH_LOUPISTS = 8;
/** A Loupist whose reads take longer than this is left out of this opening of the palette. */
const READ_BUDGET_MS = 2_000;

export type PaletteGroup = "attention" | "chats" | "following" | "map" | "principles" | "briefings" | "actions";
const GROUP_ORDER: readonly PaletteGroup[] = ["attention", "chats", "following", "map", "principles", "briefings", "actions"];

export type PaletteAction = { id: string; label: string; keys?: string; run: () => void };

type Row = { key: string; group: PaletteGroup; label: string; caption?: string; marker?: string; run: () => void };
type LoupistIndex = { nodes: KnowledgeMapNode[]; principles: { recordId: string; statement: string }[]; briefings: { runId: string; title: string; atMs: number }[] };
type Loaded = { byLoupist: Readonly<Record<string, LoupistIndex>>; followed: AssetRow[] };
const EMPTY: Loaded = { byLoupist: {}, followed: [] };

/** One Loupist's searchable names, principles and briefings, through its own read commands; null past the budget. */
async function readLoupist(bridge: LoupistBridge, loupistId: string): Promise<LoupistIndex | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<null>((resolve) => { timer = setTimeout(() => resolve(null), READ_BUDGET_MS); });
  const read = Promise.all([
    readKnowledgeMap(bridge, loupistId).catch(() => null),
    bridge.principleList({ loupistId }).catch(() => []),
    bridge.inboxList({ loupistId, limit: 20 }).catch(() => null),
  ]).then(([map, principles, inbox]): LoupistIndex => ({
    nodes: (map?.nodes ?? []).filter((node) => node.kind === "entity" && !node.archived),
    principles: principles.filter((item) => !item.archived).map((item) => ({ recordId: item.recordId, statement: item.statement })),
    briefings: (inbox?.items ?? []).filter((item) => item.kind === "briefing" && item.status === "succeeded")
      .map((item) => ({ runId: item.runId, title: item.title, atMs: item.finishedAtMs ?? item.startedAtMs })),
  }));
  try { return await Promise.race([read, late]); }
  finally { clearTimeout(timer); }
}

/** The active Loupist, then the others in sidebar order; folders that cannot be opened are skipped. */
function searchOrder(loupists: readonly LoupistSummary[], selectedLoupistId: string | null) {
  return [...loupists].filter((loupist) => loupist.availability.status === "available")
    .sort((a, b) => Number(b.id === selectedLoupistId) - Number(a.id === selectedLoupistId))
    .slice(0, SEARCH_LOUPISTS);
}

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  bridge: LoupistBridge;
  loupists: readonly LoupistSummary[];
  selectedLoupistId: string | null;
  threadsByLoupist: Readonly<Record<string, readonly ChatThread[] | undefined>>;
  attention: readonly AttentionItem[];
  actions: readonly PaletteAction[];
  /** Opened from the sidebar's "{n} more": show only what needs attention. */
  focusGroup?: PaletteGroup;
  onOpenAttention: (item: AttentionItem) => void;
  onOpenThread: (loupistId: string, threadId: string) => void;
  onOpenMapNode: (loupistId: string, node: KnowledgeMapNode) => void;
  onOpenPrinciple: (loupistId: string, recordId: string) => void;
  onOpenBriefing: (loupistId: string, runId: string) => void;
};

const matches = (query: string, ...fields: (string | undefined)[]) => !query || fields.some((field) => field?.toLowerCase().includes(query));
/**
 * A row found only through its Loupist's name needs a query of two characters or more:
 * one typed letter is in most names and would list every chat of that Loupist.
 */
const LOUPIST_NAME_QUERY = 2;
const matchesLoupist = (query: string, name: string) => [...query].length >= LOUPIST_NAME_QUERY && matches(query, name);

/**
 * The ⌘K palette (shell spec X7): one search across what needs attention,
 * chats, names on the Map, principles, briefings and app actions, over every
 * Loupist. Inside each group the active Loupist's results come first, then each
 * other Loupist's in sidebar order, captioned with its name; Enter runs the
 * highlighted row.
 */
export function CommandPalette({ open, onOpenChange, bridge, loupists, selectedLoupistId, threadsByLoupist, attention, actions, focusGroup,
  onOpenAttention, onOpenThread, onOpenMapNode, onOpenPrinciple, onOpenBriefing }: Props) {
  const { i18n: instance } = useTranslation("app");
  const openAsset = useAssetPageOpener();
  // A followed asset is listed by the name its page shows (a Korean listing in Korean) and found by either name.
  const assetName = useAssetNames(bridge, selectedLoupistId ?? "", open && Boolean(selectedLoupistId));
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<ReadonlySet<PaletteGroup>>(new Set());
  const [active, setActive] = useState(0);
  const [loaded, setLoaded] = useState<Loaded>(EMPTY);
  const listId = useId();
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActive(0);
    setExpanded(new Set(focusGroup ? [focusGroup] : []));
  }, [open, focusGroup]);

  // Names, principles and briefings are read per Loupist when the palette opens; each Loupist's
  // rows appear as its reads finish, and rows from the last opening stay until then.
  const searchedKey = useMemo(() => searchOrder(loupists, selectedLoupistId).map((loupist) => loupist.id).join("\n"), [loupists, selectedLoupistId]);
  useEffect(() => {
    if (!open) return undefined;
    let live = true;
    // The followed list is the user's, read through the active Loupist.
    if (selectedLoupistId) {
      void Promise.resolve().then(() => bridge.assetsList({ loupistId: selectedLoupistId })).catch((): AssetRow[] => []).then((followed) => {
        if (live) setLoaded((current) => ({ ...current, followed }));
      });
    }
    for (const loupistId of searchedKey ? searchedKey.split("\n") : []) {
      void readLoupist(bridge, loupistId).then((index) => {
        if (live && index) setLoaded((current) => ({ ...current, byLoupist: { ...current.byLoupist, [loupistId]: index } }));
      });
    }
    return () => { live = false; };
  }, [open, searchedKey, bridge, selectedLoupistId]);

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    const all: Row[] = [];
    for (const item of attention) {
      const what = attentionWhat(item, threadsByLoupist);
      if (matches(q, what) || matchesLoupist(q, item.loupistName)) all.push({ key: `attention:${item.kind}:${item.loupistId}:${item.chatId ?? item.recordId ?? ""}`, group: "attention", label: what, caption: attentionNamesLoupist(item) ? undefined : item.loupistName, run: () => onOpenAttention(item) });
    }
    if (focusGroup !== "attention") {
      const chats = loupists.flatMap((loupist) => (threadsByLoupist[loupist.id] ?? []).map((thread) => ({ thread, loupist, byTitle: matches(q, chatTitle(thread)) })))
        .filter(({ loupist, byTitle }) => byTitle || matchesLoupist(q, loupist.name))
        // Chats found by their title first, then the active Loupist's, then the most recent.
        .sort((a, b) => Number(b.byTitle) - Number(a.byTitle) || Number(b.loupist.id === selectedLoupistId) - Number(a.loupist.id === selectedLoupistId) || Date.parse(b.thread.updated_at) - Date.parse(a.thread.updated_at));
      // Before typing it lists recent sessions as the sidebar does; a typed search also finds the ones kept out of it.
      for (const { thread, loupist } of q ? chats : chats.filter(({ thread }) => !thread.hide_from_recent).slice(0, RECENT_CHATS)) {
        const date = Date.parse(thread.updated_at);
        all.push({ key: `chat:${loupist.id}:${thread.id}`, group: "chats", label: chatTitle(thread),
          caption: [loupist.name, Number.isFinite(date) ? formatDate(date, { year: undefined, month: "short", day: "numeric" }) : ""].filter(Boolean).join(" · "),
          run: () => onOpenThread(loupist.id, thread.id) });
      }
      if (q && selectedLoupistId && openAsset) {
        for (const asset of loaded.followed) {
          // An asset followed by name alone has no ticker; every asset opens by its id.
          const ticker = asset.reference?.ticker?.trim() ?? "";
          const label = assetName({ name: asset.name, assetId: asset.assetId, symbol: ticker || undefined, market: asset.reference?.exchange });
          if (matches(q, label, asset.name, ticker)) all.push({ key: `following:${asset.assetId}`, group: "following", label,
            caption: ticker ? tickerLabel(ticker, assetExchangeLabel(asset.reference)) : undefined,
            run: () => openAsset({ loupistId: selectedLoupistId, assetId: asset.assetId, name: label }) });
        }
      }
      // Grouped by kind, then by Loupist: the active one first, each row captioned with its Loupist.
      for (const { id: loupistId, name } of q ? searchOrder(loupists, selectedLoupistId) : []) {
        const index = loaded.byLoupist[loupistId];
        if (!index) continue;
        for (const node of index.nodes) {
          const label = entityLabel(node, instance.language);
          if (matches(q, label, node.label)) all.push({ key: `map:${loupistId}:${node.id}`, group: "map", label, marker: nodeColorKey(node), caption: name, run: () => onOpenMapNode(loupistId, node) });
        }
        for (const principle of index.principles) {
          if (matches(q, principle.statement)) all.push({ key: `principle:${loupistId}:${principle.recordId}`, group: "principles", label: principle.statement, caption: name, run: () => onOpenPrinciple(loupistId, principle.recordId) });
        }
        for (const briefing of index.briefings) {
          const date = formatDate(briefing.atMs, { year: undefined, month: "short", day: "numeric" });
          const title = scheduledTaskTitle("briefing", briefing.title);
          if (matches(q, title, date)) all.push({ key: `briefing:${loupistId}:${briefing.runId}`, group: "briefings", label: title, caption: `${name} · ${date}`, run: () => onOpenBriefing(loupistId, briefing.runId) });
        }
      }
      for (const action of actions) {
        if (matches(q, action.label)) all.push({ key: `action:${action.id}`, group: "actions", label: action.label, caption: action.keys, run: action.run });
      }
    }
    return all;
  }, [query, attention, loupists, threadsByLoupist, selectedLoupistId, loaded, actions, focusGroup, instance.language, onOpenAttention, onOpenThread, onOpenMapNode, onOpenPrinciple, onOpenBriefing, openAsset, assetName]);

  // Visible rows per group, with a "More from {group}" row when cut.
  const sections = GROUP_ORDER.flatMap((group) => {
    const inGroup = rows.filter((row) => row.group === group);
    if (!inGroup.length) return [];
    // Actions are few and are what people open the palette for; they are never folded.
    const open = expanded.has(group) || group === "actions";
    const shown = open ? inGroup : inGroup.slice(0, ROWS_PER_GROUP);
    const more: Row | null = !open && inGroup.length > ROWS_PER_GROUP
      ? { key: `more:${group}`, group, label: t("palette.moreIn", { group: t(`palette.groups.${group}`) }), run: () => setExpanded((current) => new Set([...current, group])) }
      : null;
    return [{ group, rows: more ? [...shown, more] : shown }];
  });
  const flat = sections.flatMap((section) => section.rows);
  const current = Math.min(active, Math.max(0, flat.length - 1));

  const run = (row: Row) => {
    if (row.key.startsWith("more:")) { row.run(); return; }
    onOpenChange(false);
    row.run();
  };
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!flat.length) return;
      const next = (current + (event.key === "ArrowDown" ? 1 : -1) + flat.length) % flat.length;
      setActive(next);
      listRef.current?.querySelector(`[data-index="${next}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter") {
      event.preventDefault();
      const row = flat[current];
      if (row) run(row);
    }
  };

  let index = -1;
  return <Dialog open={open} onOpenChange={onOpenChange}>
    <DialogContent className="command-palette" showCloseButton={false} aria-describedby={undefined}>
      <DialogTitle className="sr-only">{t("palette.title")}</DialogTitle>
      <input className="command-palette-input" type="search" autoFocus value={query} placeholder={t("palette.placeholder")} aria-label={t("palette.title")}
        role="combobox" aria-expanded={flat.length > 0} aria-controls={listId} aria-activedescendant={flat[current] ? `${listId}-${current}` : undefined}
        onChange={(event) => {
          setQuery(event.target.value); setActive(0);
          // New results start at the top, where the highlighted first row is.
          if (listRef.current) listRef.current.scrollTop = 0;
        }} onKeyDown={onKeyDown} />
      <ul ref={listRef} id={listId} className="command-palette-list" role="listbox" aria-label={t("palette.results")} hidden={!flat.length}>
        {sections.map((section) => <li key={section.group} role="presentation">
          <p className="command-palette-group" aria-hidden="true">{t(`palette.groups.${section.group}`)}</p>
          <ul role="group" aria-label={t(`palette.groups.${section.group}`)}>
            {section.rows.map((row) => {
              index += 1;
              const rowIndex = index;
              return <li key={row.key} id={`${listId}-${rowIndex}`} data-index={rowIndex} role="option" aria-selected={rowIndex === current}
                className="command-palette-row" data-palette-row={row.key} data-more={row.key.startsWith("more:") || undefined}
                onMouseEnter={() => setActive(rowIndex)} onMouseDown={(event) => event.preventDefault()} onClick={() => run(row)}>
                {row.marker ? <i className={`map-swatch is-${row.marker}`} aria-hidden="true" /> : null}
                <span className="command-palette-label">{row.label}</span>
                {row.caption ? <small>{row.caption}</small> : null}
              </li>;
            })}
          </ul>
        </li>)}
      </ul>
      {/* Outside the list, so a screen reader hears it as the search's outcome. */}
      <p className="command-palette-empty" role="status">{flat.length ? "" : t(focusGroup === "attention" && !query.trim() ? "palette.nothingWaiting" : "palette.empty")}</p>
    </DialogContent>
  </Dialog>;
}
