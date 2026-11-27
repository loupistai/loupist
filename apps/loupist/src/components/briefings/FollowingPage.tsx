import { useTranslation } from "@/i18n";
import { formatNumber, formatPercent } from "@/i18n/format";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { SearchIcon, XIcon } from "lucide-react";
import type { LoupistBridge, LoupistSummary, ScheduledTaskListItem } from "../../types";
import { assetTicker, type AssetLookupCandidate, type AssetPage, type AssetPricePoint, type AssetRow, type AssetStoredPrice } from "../../contracts/assets";
import { useOnline } from "../../app/useOnline";
import { commandErrorCode } from "../../lib/errors";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Input } from "../ui/input";
import { InputGroup, InputGroupAddon, InputGroupButton } from "../ui/input-group";
import { Spinner } from "../ui/spinner";
import { EmptyState } from "../ui/empty-state";
import { useAssetPageOpener } from "../assets/assetOpener";
import { PriceText } from "../market/PriceText";
import { useAssetNames } from "../assets/assetName";
import { followingBadges, learningProgramsFor, listingLabel, priceChange } from "./assetListing";
import { SegmentedControl, SegmentedControlItem } from "../ui/segmented-control";
import { FollowingAhead } from "./FollowingAhead";
import { VerdictChip } from "../assets/VerdictChip";
import { verdictLabel } from "../assets/assetText";
import { statedDate } from "../market/priceAsOf";
import "@/styles/following.css";

/** The host refuses longer queries and names (`asset_lookup`, `asset_follow_name`). */
const QUERY_MAX = 128;
const NAME_MAX = 200;

/** A followed asset as the list shows it: its name, and its reference fields or that it has none. */
type Row = { asset: AssetRow; name: string; identity: string };

/**
 * One option under the field: open a followed asset, run the listing search, follow a
 * listing it found, or follow the text as typed.
 */
type Option = { key: string } & (
  | { kind: "followed"; row: Row }
  | { kind: "search" }
  | { kind: "candidate"; candidate: AssetLookupCandidate; followed: Row | null }
  | { kind: "typed"; followed: Row | null }
);

/**
 * One listing search for one submitted text: waiting on OpenFIGI, answered, or refused.
 * The key is optional: when a saved key could not be read the public route answered,
 * and `savedKeyUnread` says so under the results.
 */
type Lookup = { query: string; state: "waiting" }
  | { query: string; state: "done"; candidates: AssetLookupCandidate[]; savedKeyUnread: boolean }
  | { query: string; state: "failed"; reason: "offline" | "connection" | "busy" | "keyRejected" | "failed" };

/** Reasons fixed in Settings › Connections, where Try again alone would fail the same way. */
const FIXED_IN_CONNECTIONS = new Set(["connection", "keyRejected"]);
const ROW_READS = 4;
const rowQueue: Array<() => void> = [];
let rowReads = 0;
/** At most four per-row reads at once. Each row fills as its own read arrives. */
function enqueueRowRead<T>(work: () => Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const run = () => {
      rowReads += 1;
      void work().then(resolve, reject).finally(() => {
        rowReads -= 1;
        rowQueue.shift()?.();
      });
    };
    if (rowReads < ROW_READS) run();
    else rowQueue.push(run);
  });
}

export type FollowingPageProps = {
  bridge: LoupistBridge;
  loupist: LoupistSummary;
  /** Opens Settings › Connections, where Asset lookup is switched on. */
  onOpenConnections?: (connectionId?: string) => void;
  /** Puts a request into Bot's composer without sending it. */
  onAskInChat?: (text: string) => void;
  /** Opens a scheduled task. The shell owns that navigation. */
  onOpenTask?: (taskId: string) => void;
  /** Bot's "Add assets to follow" opened the page: the field takes focus. */
  focusSearch?: boolean;
};

/**
 * Following: one field and one list. Typing filters the followed list here and sends
 * nothing. Enter, or "Search listings", sends the submitted text once to OpenFIGI
 * through the host (`asset_lookup`) and lists its candidates. Picking one follows it
 * with that listing's fields as its reference. The last option follows by name alone
 * or opens a followed asset with that name. The host starts one short delegated run
 * per burst of follows to find each new asset's price.
 */
export function FollowingPage({ bridge, loupist, onOpenConnections, onAskInChat, onOpenTask, focusSearch }: FollowingPageProps) {
  const { t, i18n: { language } } = useTranslation(["briefings", "assets", "connections"]);
  const loupistId = loupist.id;
  const names = useAssetNames(bridge, loupistId);
  const openAsset = useAssetPageOpener();
  const online = useOnline();

  const [assets, setAssets] = useState<AssetRow[] | null>(null);
  const [prices, setPrices] = useState(new Map<string, AssetStoredPrice>());
  const [tasks, setTasks] = useState<ScheduledTaskListItem[]>([]);
  const [listFailed, setListFailed] = useState(false);
  const [listRevision, setListRevision] = useState(0);
  const [query, setQuery] = useState("");
  const [composing, setComposing] = useState(false);
  const [lookup, setLookup] = useState<Lookup | null>(null);
  const [active, setActive] = useState<string | null>(null);
  const [picks, setPicks] = useState<Readonly<Record<string, "adding" | "failed">>>({});
  const [removals, setRemovals] = useState<Readonly<Record<string, "pending" | "failed">>>({});
  const [stopAsk, setStopAsk] = useState<Row | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [view, setView] = useState<"list" | "ahead">("list");
  const fieldRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  /** A newer submit or a cleared field drops an older answer. */
  const generation = useRef(0);
  /** After an add or a removal: the row to bring into view or focus, or the field. */
  const after = useRef<{ key: string; focus: boolean } | "field" | null>(null);
  const listboxId = useId();
  const hintId = useId();

  // A follow, an unfollow or a stored price change elsewhere reads the list again.
  useEffect(() => bridge.onAssetsChanged?.(() => setListRevision(value => value + 1)), [bridge]);
  useEffect(() => {
    let live = true;
    void bridge.watchlistQuotes({ loupistId })
      .then(quotes => { if (live) setPrices(new Map(quotes.map(({ assetId, price }) => [assetId, price]))); })
      .catch(() => { if (live) setPrices(new Map()); });
    void bridge.scheduledTasksList({ loupist_id: loupistId })
      .then(next => { if (live) setTasks(next); })
      .catch(() => { if (live) setTasks([]); });
    void Promise.resolve().then(() => bridge.assetsList({ loupistId }))
      .then(next => { if (live) { setAssets(next); setListFailed(false); } })
      .catch(() => { if (live) setListFailed(true); });
    return () => { live = false; };
  }, [bridge, loupistId, listRevision]);

  const collator = useMemo(() => new Intl.Collator(language), [language]);
  const listing = useMemo(() => listingLabel(language), [language]);
  const rows = useMemo((): Row[] => (assets ?? [])
    .map(asset => {
      const ticker = assetTicker(asset);
      const name = names({ name: asset.name, assetId: asset.assetId, symbol: ticker ?? undefined, market: asset.reference?.exchange });
      return { asset, name, identity: (asset.reference ? listing(asset.reference) : null) ?? t("following.nameOnly") };
    })
    .sort((a, b) => collator.compare(a.name, b.name) || collator.compare(a.asset.assetId, b.asset.assetId)), [assets, collator, listing, names, t]);

  const trimmed = query.trim();
  const submitted = lookup?.query === trimmed ? lookup : null;
  const candidates = useMemo(() => submitted?.state === "done" ? submitted.candidates : [], [submitted]);
  const typedFollowed = useMemo(() => {
    if (!trimmed) return null;
    const lower = trimmed.toLowerCase();
    const exact = (name: string) => name.trim().toLowerCase() === lower;
    // Match the saved name first, using the same ID tie break as the host.
    const saved = rows.reduce<Row | null>((match, row) =>
      exact(row.asset.name) && (!match || row.asset.assetId < match.asset.assetId) ? row : match, null);
    return saved ?? rows.find(row => exact(row.name)) ?? null;
  }, [rows, trimmed]);

  const options = useMemo((): Option[] => {
    if (!trimmed) return [];
    const needle = trimmed.toLocaleLowerCase();
    // Typing filters what is followed already; nothing leaves the Mac until Enter.
    const local: Option[] = rows.filter(row => [row.name, row.asset.name, row.asset.reference?.ticker ?? ""].some(text => text.toLocaleLowerCase().includes(needle)))
      .map(row => ({ key: `f:${row.asset.assetId}`, kind: "followed", row }));
    const shown = new Set(local.map(option => option.kind === "followed" ? option.row.asset.assetId : ""));
    // A listing already followed is that row; it shows once, among the followed matches when typing found it.
    const found: Option[] = candidates.flatMap((candidate): Option[] => {
      const followed = rows.find(row => row.asset.reference?.figi === candidate.figi) ?? null;
      return followed && shown.has(followed.asset.assetId) ? [] : [{ key: `c:${candidate.figi}`, kind: "candidate", candidate, followed }];
    });
    return [
      ...local,
      ...(submitted ? [] : [{ key: "search", kind: "search" } as const]),
      ...found,
      { key: "typed", kind: "typed", followed: typedFollowed },
    ];
  }, [trimmed, rows, candidates, submitted, typedFollowed]);

  // Following waits until the list is known, so "Following" on a result is never wrong; opening and searching need nothing.
  const pickable = (option: Option) => option.kind === "followed" || option.kind === "search" || option.followed !== null
    || (assets !== null && picks[option.key] !== "adding");
  const navigable = options.filter(pickable);
  const activeOption = navigable.find(option => option.key === active) ?? null;
  const optionId = (option: Option) => `${listboxId}-${options.indexOf(option)}`;

  // The field takes focus only when there is nothing to manage yet, or when Bot's card sent the user here.
  const focusedOnOpen = useRef(false);
  useEffect(() => {
    if (focusedOnOpen.current || (!focusSearch && assets === null)) return;
    focusedOnOpen.current = true;
    if (focusSearch || assets?.length === 0) fieldRef.current?.focus();
  }, [focusSearch, assets]);

  // After an add, the new row comes into view; after a removal, focus moves to a neighbouring row or the field.
  useEffect(() => {
    const target = after.current;
    if (!target || trimmed) return;
    after.current = null;
    if (target === "field") { fieldRef.current?.focus(); return; }
    const row = [...listRef.current?.querySelectorAll<HTMLElement>("[data-row-key]") ?? []].find(item => item.dataset.rowKey === target.key);
    row?.scrollIntoView?.({ block: "nearest" });
    if (!target.focus) return;
    const open = row?.querySelector<HTMLElement>(".following-open");
    if (open) open.focus(); else fieldRef.current?.focus();
  }, [rows, trimmed]);

  const clearQuery = () => { generation.current += 1; setQuery(""); setActive(null); setLookup(null); fieldRef.current?.focus(); };

  const lookupFailureMessage = (reason: Extract<Lookup, { state: "failed" }>["reason"]) =>
    t(reason === "offline" && typedFollowed ? "following.lookup.offlineFollowed" : `following.lookup.${reason}`);

  /** Sends the submitted text once; a second Enter on the same answered text sends nothing. */
  const search = async () => {
    const text = trimmed;
    if (!text || (submitted && submitted.state !== "failed")) return;
    const current = ++generation.current;
    if (!online) {
      setLookup({ query: text, state: "failed", reason: "offline" });
      setAnnouncement(t(typedFollowed ? "following.lookup.offlineFollowed" : "following.lookup.offline"));
      return;
    }
    setLookup({ query: text, state: "waiting" });
    setActive(null);
    try {
      const { candidates: found, savedKeyUnread } = await bridge.assetLookup({ loupistId, query: text });
      if (current !== generation.current) return;
      setLookup({ query: text, state: "done", candidates: found, savedKeyUnread });
      const count = t("following.announce.count", { count: found.length, formattedCount: formatNumber(found.length) });
      setAnnouncement(savedKeyUnread ? `${count} ${t("following.lookup.keyUnreadable")}` : count);
    } catch (cause) {
      if (current !== generation.current) return;
      const code = commandErrorCode(cause);
      const reason = code === "credential_rejected" ? "keyRejected"
        : code === "offline" ? "offline"
        : code === "connection_unavailable" || code === "not_configured" ? "connection"
        : code === "rate_limited" ? "busy"
        : "failed";
      setLookup({ query: text, state: "failed", reason });
      setAnnouncement(lookupFailureMessage(reason));
    }
  };

  const followed = (row: AssetRow, name: string) => {
    // A listing another session saved meanwhile is the same row, not a second one.
    setAssets(current => [...(current ?? []).filter(item => item.assetId !== row.assetId), row]);
    generation.current += 1;
    setQuery(""); setActive(null); setLookup(null);
    after.current = { key: row.assetId, focus: false };
    fieldRef.current?.focus();
    setAnnouncement(t("following.announce.added", { name, loupist: loupist.name }));
  };

  const follow = async (option: Option & { kind: "candidate" | "typed" }) => {
    const name = option.kind === "candidate" ? option.candidate.name : trimmed.slice(0, NAME_MAX);
    setPicks(current => ({ ...current, [option.key]: "adding" }));
    try {
      const row = await bridge.assetFollowName(option.kind === "candidate"
        ? { loupistId, name, reference: { figi: option.candidate.figi, ticker: option.candidate.ticker, exchange: option.candidate.exchange, exchangeName: option.candidate.exchangeName, securityType: option.candidate.securityType } }
        : { loupistId, name });
      setPicks(current => { const { [option.key]: _, ...rest } = current; return rest; });
      followed(row, row.name || name);
    } catch {
      setPicks(current => ({ ...current, [option.key]: "failed" }));
      setAnnouncement(t("following.announce.addFailed", { name }));
    }
  };

  const activate = (option: Option) => {
    if (!pickable(option)) return;
    if (option.kind === "search") { void search(); return; }
    const row = option.kind === "followed" ? option.row : option.followed;
    if (row) openAsset?.({ loupistId, assetId: row.asset.assetId, name: row.name });
    else if (option.kind !== "followed") void follow(option);
  };

  const move = (step: 1 | -1) => {
    if (!navigable.length) return;
    const index = navigable.findIndex(option => option.key === active);
    const next = navigable[index < 0 ? (step === 1 ? 0 : navigable.length - 1) : (index + step + navigable.length) % navigable.length]!;
    setActive(next.key);
    document.getElementById(optionId(next))?.scrollIntoView?.({ block: "nearest" });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    // Keys during composition belong to the input method: its Enter never searches.
    if (event.nativeEvent.isComposing || composing) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      if (!trimmed) return;
      event.preventDefault();
      move(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Enter") {
      if (!trimmed) return;
      event.preventDefault();
      // Enter picks the active option; otherwise it searches listings for the text.
      if (activeOption) activate(activeOption);
      else void search();
    } else if (event.key === "Escape") {
      if (activeOption) { event.preventDefault(); setActive(null); }
      else if (query) { event.preventDefault(); clearQuery(); }
    }
  };

  const stopFollowing = async (row: Row, pauseDaily: boolean) => {
    const id = row.asset.assetId;
    setStopAsk(null);
    setRemovals(current => ({ ...current, [id]: "pending" }));
    try {
      if (pauseDaily) {
        for (const task of learningProgramsFor(id, tasks)) {
          await bridge.scheduledTaskSetStatus({ loupist_id: loupistId, task_id: task.id, expected_revision: task.revision, status: "paused" });
        }
      }
      await bridge.assetUnfollow({ loupistId, assetId: id });
      const index = rows.findIndex(item => item.asset.assetId === id);
      const neighbour = rows[index + 1] ?? rows[index - 1];
      after.current = neighbour ? { key: neighbour.asset.assetId, focus: true } : "field";
      setAssets(current => (current ?? []).filter(item => item.assetId !== id));
      setRemovals(current => { const { [id]: _, ...rest } = current; return rest; });
    } catch {
      setRemovals(current => ({ ...current, [id]: "failed" }));
      setAnnouncement(t("following.stop.failed", { name: row.name }));
    }
  };

  const listFailure = <div role="alert" className="inline-error following-line" data-following-list-failed>
    <span>{t("following.loadFailed")}</span>{" "}<Button variant="outline" size="sm" onClick={() => setListRevision(value => value + 1)}>{t("retry")}</Button>
  </div>;

  const actionName = (option: Option) => {
    switch (option.kind) {
      case "followed": return t("following.action.open", { name: option.row.name });
      case "search": return t("following.action.search", { query: trimmed });
      case "typed": return option.followed ? t("following.action.open", { name: option.followed.name })
        : t("following.action.typed", { query: trimmed });
      case "candidate": return option.followed ? t("following.action.open", { name: option.followed.name })
        : t("following.action.follow", { name: option.candidate.name, listing: listing(option.candidate) ?? option.candidate.figi });
    }
  };

  const optionRow = (option: Option) => {
    const state = (option.kind === "typed" || option.kind === "candidate") && !option.followed ? picks[option.key] : undefined;
    const id = optionId(option);
    const [label, identity, status] = option.kind === "followed" ? [option.row.name, option.row.identity, t("following.result.followed")]
      : option.kind === "search" ? [t("following.search.listings", { query: trimmed }), null, null]
        : option.kind === "typed" ? (option.followed
          ? [t("following.typedOpen", { name: option.followed.name }), option.followed.identity, t("following.typedFollowed")]
          : [t("following.typed", { query: trimmed }), t("following.typedHint", { loupist: loupist.name }), null])
          : [option.candidate.name, listing(option.candidate), option.followed ? t("following.result.followed") : null];
    const statusText = state === "adding" ? t("following.result.adding") : state === "failed" ? t("following.result.addFailed")
      : status ?? (option.kind === "candidate" ? t("following.result.follow") : null);
    return <div key={option.key} id={id} role="option" className="following-option"
      aria-selected={option.key === activeOption?.key} aria-disabled={!pickable(option) || undefined} aria-label={actionName(option)}
      data-kind={option.kind} data-pick={state} data-figi={option.kind === "candidate" ? option.candidate.figi : undefined}
      onMouseDown={event => event.preventDefault()} onClick={() => activate(option)}>
      {option.kind === "search" ? <SearchIcon aria-hidden="true" className="following-create-icon size-4" /> : null}
      <span className="following-names">
        <span className="following-name" title={label}>{label}</span>
        {identity ? <span className="following-identity gg-num">{identity}</span> : null}
      </span>
      {statusText ? <span className="following-status" data-tone={state === "failed" ? "negative" : undefined} data-action={(option.kind === "candidate" && !option.followed && !state) || undefined}>
        {state === "adding" ? <Spinner aria-hidden="true" role="presentation" /> : null}
        {statusText}{state === "failed" ? <> · <span className="following-retry">{t("retry")}</span></> : null}
      </span> : null}
    </div>;
  };

  const askBot = onAskInChat && !typedFollowed ? <Button variant="outline" size="sm" onClick={() => onAskInChat(t("following.askBot.draft", { query: trimmed }))}>{t("following.askBot.action")}</Button> : null;
  const lookupLine = () => {
    if (!submitted) return <p className="following-line following-note" data-lookup="idle">{t("following.search.enter")}</p>;
    if (submitted.state === "waiting") return <p className="following-line" role="status" data-lookup="waiting"><Spinner aria-hidden="true" role="presentation" />{t("following.lookup.searching")}</p>;
    if (submitted.state === "done") {
      const answered = submitted.candidates.length ? <p className="following-line following-note" data-lookup="done">{t("following.lookup.from")}</p>
        : <div className="following-line" data-lookup="none"><span>{t(typedFollowed ? "following.lookup.noneFollowed" : "following.lookup.none")}</span><span className="following-line-actions">{askBot}</span></div>;
      // The public route answered; the saved key that could not be read is fixed in Connections.
      return submitted.savedKeyUnread ? <>
        {answered}
        <div className="following-line" data-lookup-key="unread">
          <span>{t("following.lookup.keyUnreadable")}</span>
          {onOpenConnections ? <span className="following-line-actions"><Button variant="outline" size="sm" onClick={() => onOpenConnections("openfigi.lookup")}>{t("following.openConnections")}</Button></span> : null}
        </div>
      </> : answered;
    }
    const inConnections = FIXED_IN_CONNECTIONS.has(submitted.reason);
    return <div className="following-line" data-lookup={submitted.reason}>
      <span>{lookupFailureMessage(submitted.reason)}</span>
      <span className="following-line-actions">
        {inConnections ? null : <Button variant="outline" size="sm" onClick={() => void search()}>{t("retry")}</Button>}
        {inConnections && onOpenConnections ? <Button variant="outline" size="sm" onClick={() => onOpenConnections("openfigi.lookup")}>{t("following.openConnections")}</Button> : null}
      </span>
    </div>;
  };

  const resting = assets === null
    ? listFailed ? listFailure : <p className="following-line" role="status" data-following-loading><Spinner aria-hidden="true" role="presentation" />{t("following.loading")}</p>
    : <>
      {listFailed ? listFailure : null}
      {rows.length === 0 ? <EmptyState className="following-empty" title={t("following.emptyTitle")} description={t("following.emptyBody")} />
        : <ul className="following-list" aria-label={t("following.listLabel")}>
          {rows.map(row => {
            const id = row.asset.assetId;
            const removal = removals[id];
            const price = prices.get(id);
            const content = <>
              {/* A long name ends in an ellipsis; the pointer shows it whole (the button's name already has it). */}
              <span className="following-name" title={row.name}>{row.name}</span>
              <span className="following-identity gg-num">{row.identity}</span>
            </>;
            const lead = openAsset
              ? <button type="button" className="following-open" onClick={() => openAsset({ loupistId, assetId: id, name: row.name })}>{content}</button>
              : <span className="following-open">{content}</span>;
            return <li key={id} className="following-row" data-row-key={id} data-ticker={row.asset.reference?.ticker ?? undefined} data-removing={removal === "pending" || undefined}>
              <FollowingFacts bridge={bridge} loupistId={loupistId} assetId={id} price={price} tasks={tasks} lead={lead} />
              <Button variant="ghost" size="sm" className="following-stop" aria-label={t("following.stop.label", { name: row.name })} aria-disabled={removal === "pending" || undefined}
                onClick={() => { if (removal === "pending") return; if (learningProgramsFor(id, tasks).length) setStopAsk(row); else void stopFollowing(row, false); }}>{t("following.stop.action")}</Button>
              {removal === "failed" ? <p role="alert" className="following-row-error">
                {t("following.stop.failed", { name: row.name })}{" "}<Button variant="link" size="sm" onClick={() => { if (learningProgramsFor(id, tasks).length) setStopAsk(row); else void stopFollowing(row, false); }}>{t("retry")}</Button>
              </p> : null}
            </li>;
          })}
        </ul>}
    </>;

  return <section className="following-page" data-testid="following-page" aria-labelledby="current-location">
    <SegmentedControl className="following-view" value={view} onValueChange={value => setView(value as "list" | "ahead")} aria-label={t("following.view.label")}>
      <SegmentedControlItem value="list">{t("following.view.list")}</SegmentedControlItem>
      <SegmentedControlItem value="ahead">{t("following.view.ahead")}</SegmentedControlItem>
    </SegmentedControl>
    <div className="following-search">
      <InputGroup className="following-field">
        <Input ref={fieldRef} type="text" data-slot="input-group-control" className="following-input" value={query} maxLength={QUERY_MAX}
          role="combobox" aria-autocomplete="list" aria-expanded={Boolean(trimmed) && options.length > 0} aria-controls={trimmed ? listboxId : undefined}
          aria-activedescendant={activeOption ? optionId(activeOption) : undefined}
          aria-label={t("following.search.label")} aria-describedby={hintId} placeholder={t("following.search.label")} autoComplete="off" spellCheck={false}
          onChange={event => { setQuery(event.target.value); setActive(null); }}
          onCompositionStart={() => setComposing(true)} onCompositionEnd={event => { setComposing(false); setQuery(event.currentTarget.value); }}
          onKeyDown={onKeyDown} />
        {query ? <InputGroupAddon align="inline-end">
          <InputGroupButton size="icon-xs" aria-label={t("following.search.clear")} onClick={clearQuery}><XIcon aria-hidden="true" /></InputGroupButton>
        </InputGroupAddon> : null}
      </InputGroup>
    </div>
    <div ref={listRef} className="following-body" aria-busy={assets === null && !listFailed}>
      {view === "ahead" && !trimmed ? <FollowingAhead bridge={bridge} loupistId={loupistId} assets={assets} onOpenTask={onOpenTask} /> : trimmed ? <>
        {listFailed && assets === null ? listFailure : null}
        <div id={listboxId} role="listbox" aria-label={t("following.search.results")} className="following-results">
          {options.map(optionRow)}
        </div>
        {lookupLine()}
      </> : resting}
    </div>
    {/* What typing and Enter do, for a screen reader; sighted users read it under the results. */}
    <p id={hintId} className="sr-only">{t("following.search.hint")}</p>
    <p className="sr-only" aria-live="polite">{announcement}</p>
    <UnfollowDailyDialog name={stopAsk?.name ?? ""} open={stopAsk !== null} onCancel={() => setStopAsk(null)} onConfirm={pause => { if (stopAsk) void stopFollowing(stopAsk, pause); }} />
  </section>;
}

export function UnfollowDailyDialog({ name, open, onCancel, onConfirm }: {
  name: string; open: boolean; onCancel: () => void; onConfirm: (pauseDaily: boolean) => void;
}) {
  const { t } = useTranslation("briefings");
  const [pauseDaily, setPauseDaily] = useState(true);
  useEffect(() => { if (open) setPauseDaily(true); }, [open]);
  return <Dialog open={open} onOpenChange={next => { if (!next) onCancel(); }}>
    <DialogContent>
      <DialogHeader><DialogTitle>{t("following.stop.confirm", { name })}</DialogTitle></DialogHeader>
      <DialogBody>
        <label className="following-pause-daily">
          <input type="checkbox" checked={pauseDaily} onChange={event => setPauseDaily(event.target.checked)} />
          {t("following.stop.pauseDaily", { name })}
        </label>
      </DialogBody>
      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onCancel}>{t("following.stop.cancel")}</Button>
        <Button type="button" onClick={() => onConfirm(pauseDaily)}>{t("following.stop.action")}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>;
}

function FollowingFacts({ bridge, loupistId, assetId, price, tasks, lead }: {
  bridge: LoupistBridge; loupistId: string; assetId: string; price?: AssetStoredPrice; tasks: readonly ScheduledTaskListItem[]; lead: ReactNode;
}) {
  const { t } = useTranslation(["briefings", "assets"]);
  const [series, setSeries] = useState<AssetPricePoint[] | null | undefined>(undefined);
  const [page, setPage] = useState<AssetPage | "failed" | undefined>(undefined);
  const [showWait, setShowWait] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let live = true;
    setSeries(undefined);
    setPage(undefined);
    setShowWait(false);
    const timer = window.setTimeout(() => { if (live) setShowWait(true); }, 300);
    let seriesDone = false;
    let pageDone = false;
    const settled = () => { if (seriesDone && pageDone) window.clearTimeout(timer); };
    void enqueueRowRead(() => bridge.assetPriceSeries({ loupistId, assetId, storedOnly: true }))
      .then(points => { if (live) setSeries(points); }, () => { if (live) setSeries(null); })
      .finally(() => { seriesDone = true; settled(); });
    void enqueueRowRead(() => bridge.assetPageGet({ loupistId, assetId, storedOnly: true }))
      .then(view => { if (live) setPage(view); }, () => { if (live) setPage("failed"); })
      .finally(() => { pageDone = true; settled(); });
    return () => { live = false; window.clearTimeout(timer); };
  }, [bridge, loupistId, assetId, attempt]);
  const pending = series === undefined || page === undefined;
  const read = page !== "failed" && page ? page.read : null;
  const setup = page !== "failed" && page ? page.priceSetupRunning : false;
  const badges = page === "failed" ? [] : followingBadges(assetId, tasks);
  const change = price && series ? priceChange(price, series) : null;
  const unseen = Boolean(read?.latestMove && read.latestMove.seenAtMs == null);
  const priceDate = price ? (price.kind ? `${t(`price.kind.${price.kind}`, { ns: "assets" })} · ${statedDate(price.date)}` : statedDate(price.date)) : "";
  const quote = !price && setup ? <p className="following-quote-meta">{t("following.priceFinding")}</p>
    : !price && pending && showWait ? <p className="following-quote-meta" role="status"><Spinner aria-hidden="true" />{t("following.priceFinding")}</p>
    : !price && !pending ? <p className="following-quote-meta">{t("following.priceNone")}</p>
    : price ? <p className="following-quote">
      <span title={price.currencySource === "none" ? t("following.currencyUnstated") : undefined}>
        <PriceText value={price.value} currency={price.currencySource === "none" ? undefined : price.currency ?? undefined} digits="auto" />
      </span>
      <small className="following-quote-meta">
        {series === undefined && showWait ? <Spinner aria-hidden="true" /> : null}
        {change ? <span className="following-change" data-direction={change.delta > 0 ? "up" : change.delta < 0 ? "down" : "flat"} title={t("following.changeVersus", { date: statedDate(change.date) })}>
          {change.delta > 0 ? "↑ " : change.delta < 0 ? "↓ " : ""}{formatNumber(change.delta, { signDisplay: "always", minimumFractionDigits: 2, maximumFractionDigits: 2 })}
          {change.percent === null ? "" : ` (${formatPercent(change.percent, { signDisplay: "always", minimumFractionDigits: 1, maximumFractionDigits: 1 })})`}
        </span> : null}
        {priceDate ? <span className="following-price-date">{change ? " · " : ""}{priceDate}</span> : null}
      </small>
    </p> : null;
  const chipName = read && unseen ? `${verdictLabel(read.verdict)} · ${t("following.newMove")}` : undefined;
  return <>
    <div className="following-lead">
      {lead}
      {read ? <span className="following-thesis" role={unseen ? "img" : undefined} aria-label={chipName}>
        {unseen ? <i className="following-move-dot" aria-hidden="true" /> : null}
        <span aria-hidden={unseen || undefined}><VerdictChip verdict={read.verdict} /></span>
      </span> : null}
    </div>
    <span className="following-badges">
      {badges.map(badge => <span key={badge.kind} className="following-badge" data-failing={badge.failing || undefined}>
        {badge.kind === "watch" && badge.count > 1 ? t("following.badge.watches", { count: badge.count }) : t(`following.badge.${badge.kind}`)}
      </span>)}
    </span>
    {quote}
    {page === "failed" || series === null ? <Button variant="link" size="sm" className="following-row-retry" onClick={() => setAttempt(value => value + 1)}>{t("retry")}</Button> : null}
  </>;
}
