import { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { SearchIcon, XIcon } from "lucide-react";
import type { LoupistBridge, LoupistSummary } from "../../types";
import { assetTicker, type AssetLookupCandidate, type AssetRow, type AssetStoredPrice } from "../../contracts/assets";
import { useOnline } from "../../app/useOnline";
import { commandErrorCode } from "../../lib/errors";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { InputGroup, InputGroupAddon, InputGroupButton } from "../ui/input-group";
import { Spinner } from "../ui/spinner";
import { EmptyState } from "../ui/empty-state";
import { useAssetPageOpener } from "../assets/assetOpener";
import { PriceText } from "../market/PriceText";
import { useAssetNames } from "../assets/assetName";
import { listingLabel } from "./assetListing";
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

export type FollowingPageProps = {
  bridge: LoupistBridge;
  loupist: LoupistSummary;
  /** Opens Settings › Connections, where Asset lookup is switched on. */
  onOpenConnections?: (connectionId?: string) => void;
  /** Puts a request into Bot's composer without sending it. */
  onAskInChat?: (text: string) => void;
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
export function FollowingPage({ bridge, loupist, onOpenConnections, onAskInChat, focusSearch }: FollowingPageProps) {
  const { t, i18n: { language } } = useTranslation(["briefings", "assets", "connections"]);
  const loupistId = loupist.id;
  const names = useAssetNames(bridge, loupistId);
  const openAsset = useAssetPageOpener();
  const online = useOnline();

  const [assets, setAssets] = useState<AssetRow[] | null>(null);
  const [prices, setPrices] = useState(new Map<string, AssetStoredPrice>());
  const [listFailed, setListFailed] = useState(false);
  const [listRevision, setListRevision] = useState(0);
  const [query, setQuery] = useState("");
  const [composing, setComposing] = useState(false);
  const [lookup, setLookup] = useState<Lookup | null>(null);
  const [active, setActive] = useState<string | null>(null);
  const [picks, setPicks] = useState<Readonly<Record<string, "adding" | "failed">>>({});
  const [removals, setRemovals] = useState<Readonly<Record<string, "pending" | "failed">>>({});
  const [announcement, setAnnouncement] = useState("");
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

  const stopFollowing = async (row: Row) => {
    const id = row.asset.assetId;
    setRemovals(current => ({ ...current, [id]: "pending" }));
    try {
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
            return <li key={id} className="following-row" data-row-key={id} data-ticker={row.asset.reference?.ticker ?? undefined} data-removing={removal === "pending" || undefined}>
              {openAsset ? <button type="button" className="following-open" onClick={() => openAsset({ loupistId, assetId: id, name: row.name })}>{content}</button>
                : <span className="following-open">{content}</span>}
              {price ? <PriceText value={price.value} currency={price.currency ?? undefined} digits="auto" /> : null}
              <Button variant="ghost" size="sm" className="following-stop" aria-label={t("following.stop.label", { name: row.name })} aria-disabled={removal === "pending" || undefined}
                onClick={() => { if (removal !== "pending") void stopFollowing(row); }}>{t("following.stop.action")}</Button>
              {removal === "failed" ? <p role="alert" className="following-row-error">
                {t("following.stop.failed", { name: row.name })}{" "}<Button variant="link" size="sm" onClick={() => void stopFollowing(row)}>{t("retry")}</Button>
              </p> : null}
            </li>;
          })}
        </ul>}
    </>;

  return <section className="following-page" data-testid="following-page" aria-labelledby="current-location">
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
      {trimmed ? <>
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
  </section>;
}
