import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { ArrowRightIcon, XIcon } from "lucide-react";
import type { LoupistBridge } from "../../types";
import { assetTicker, type AssetRow, type StoredAssetQuote } from "../../contracts/assets";
import type { LoupistAvatarIdentity } from "../../lib/loupistAvatar";
import { providerName } from "../../lib/marketProviderName";
import { LoupistAvatar } from "../app/LoupistAvatar";
import { useAssetNames } from "../assets/assetName";
import { PriceText } from "../market/PriceText";
import { priceAsOf } from "../market/priceAsOf";
import { useGo } from "../navigation/Open";
import { joinMeta } from "./BotPost";
import "@/styles/bot-thread.css";

const t = i18n.getFixedT(null, "chat");

const MAX_TILES = 4;
const MAX_ASKS = 4;
const PRINCIPLE_HIDDEN_KEY = "loupist.bot.principleHidden.v1";

type Props = {
  bridge: LoupistBridge;
  loupistId: string;
  loupist: LoupistAvatarIdentity;
  /** Puts the text into the composer; it never sends. */
  onAsk: (text: string) => void;
  accountConnected?: boolean;
  onConnectAccount?: () => void;
  /** "Add assets to follow": opens Following with its search field focused. */
  onOpenFollowing?: () => void;
  /** Bumped each time Bot is shown again, so a step done elsewhere drops out. */
  refreshRevision?: number;
};

type Read = {
  watchlist: AssetRow[];
  quotes: StoredAssetQuote[];
  briefingOn: boolean | null;
  principles: number | null;
};

type Followed = { key: string; name: string; quote?: StoredAssetQuote };

type Card = { key: string; label: string; hint?: string; run: () => void };

/**
 * By the Mac's local hour: morning from 05:00, afternoon from 12:00, evening from 18:00 to
 * midnight. Neither English nor Korean greets the small hours by name ("Good evening" at 03:38
 * read wrong), so 00:00 to 05:00 takes the neutral heading an empty research session opens with.
 */
function greetingKey(hour: number) {
  if (hour < 5) return "today.askGreeting";
  if (hour < 12) return "bot.greeting.morning";
  if (hour < 18) return "bot.greeting.afternoon";
  return "bot.greeting.evening";
}

function principleHidden(loupistId: string) {
  try { return window.localStorage.getItem(`${PRINCIPLE_HIDDEN_KEY}:${loupistId}`) === "1"; } catch { return false; }
}
function hidePrinciple(loupistId: string) {
  try { window.localStorage.setItem(`${PRINCIPLE_HIDDEN_KEY}:${loupistId}`, "1"); } catch { /* A private window keeps it for this view only. */ }
}

/**
 * The empty Bot: the Loupist greets the user by the time of day, offers things to ask
 * as cards that fill the composer, and shows the stored price of each followed asset
 * as its last replay stated it, with its date and provider. No model call. Setup
 * steps that remain are among the cards and drop out once done.
 */
export function BotGreeting({ bridge, loupistId, loupist, onAsk, accountConnected = true, onConnectAccount, onOpenFollowing, refreshRevision = 0 }: Props) {
  useTranslation("chat");
  const go = useGo();
  // The greeting and date are those of the moment Bot is shown, again each time it is shown.
  const [opened, setOpened] = useState(() => ({ nowMs: Date.now(), revision: refreshRevision }));
  if (opened.revision !== refreshRevision) setOpened({ nowMs: Date.now(), revision: refreshRevision });
  const [read, setRead] = useState<Read | null>(null);
  const [principleOff, setPrincipleOff] = useState(() => principleHidden(loupistId));
  // Hide takes its own button away: focus moves to the cards above it.
  const asksRef = useRef<HTMLUListElement>(null);
  const tileIds = useId();
  // `assets-changed` (a follow, or a stored price a replay changed) reads the list and its prices again.
  const [changedRevision, setChangedRevision] = useState(0);
  useEffect(() => bridge.onAssetsChanged?.(() => setChangedRevision(value => value + 1)), [bridge]);

  useEffect(() => {
    let current = true;
    void Promise.all([
      Promise.resolve().then(() => bridge.assetsList({ loupistId })).catch(() => [] as AssetRow[]),
      Promise.resolve().then(() => bridge.watchlistQuotes({ loupistId })).catch(() => [] as StoredAssetQuote[]),
      bridge.briefingSettingsGet({ loupistId }).then((settings) => settings.enabled, () => null),
      Promise.resolve().then(() => bridge.principleList?.({ loupistId })).then((list) => list?.length ?? null, () => null),
    ]).then(([watchlist, quotes, briefingOn, principles]) => {
      if (current) setRead({ watchlist, quotes, briefingOn, principles });
    });
    return () => { current = false; };
  }, [bridge, loupistId, refreshRevision, changedRevision]);

  // A follow and its stored price are one asset by id, named as its asset page names it ("한울정밀" in Korean).
  const names = useAssetNames(bridge, loupistId, Boolean(read?.watchlist.length));
  const followed = useMemo<Followed[]>(() => (read?.watchlist ?? []).map((entry) => ({
    key: entry.assetId,
    name: names({ name: entry.name, assetId: entry.assetId, symbol: assetTicker(entry) ?? undefined, market: entry.reference?.exchange }),
    quote: read?.quotes.find((item) => item.assetId === entry.assetId),
  })), [names, read]);

  const priced = followed.filter((item) => item.quote).slice(0, MAX_TILES);

  const cards: Card[] = [];
  if (read) {
    // The card names the question; the composer gets it as the user's own request.
    const ask = (key: string, label: string, hint: string, prompt: string): Card => ({ key, label, hint, run: () => onAsk(prompt) });
    if (!accountConnected && onConnectAccount) cards.push({ key: "connect", label: t("bot.greeting.setup.connect.label"), hint: t("bot.greeting.setup.connect.hint"), run: onConnectAccount });
    const setupBriefing: Card = { key: "setup-briefing", label: t("bot.greeting.setup.briefing.label"), hint: t("bot.greeting.setup.briefing.hint"), run: () => onAsk(t("today.setup.briefing.insert")) };
    if (!followed.length) {
      if (onOpenFollowing) cards.push({ key: "follow", label: t("bot.greeting.setup.follow.label"), hint: t("bot.greeting.setup.follow.hint"), run: onOpenFollowing });
      if (read.briefingOn === false) cards.push(setupBriefing);
      cards.push(ask("markets", t("bot.greeting.ask.markets.label"), t("bot.greeting.ask.markets.hint"), t("bot.greeting.ask.markets.prompt")));
    } else {
      const lead = followed[0]!;
      cards.push(ask("status", t("bot.greeting.ask.status.label", { name: lead.name }), t("bot.greeting.ask.status.hint"), t("bot.greeting.ask.status.prompt", { name: lead.name })));
      cards.push(read.briefingOn === false ? setupBriefing
        : ask("briefing", t("bot.greeting.ask.briefing.label"), t("bot.greeting.ask.briefing.hint", { count: followed.length, formattedCount: formatNumber(followed.length) }), t("bot.greeting.ask.briefing.prompt")));
      const next = followed.find((item) => item !== lead);
      if (next) cards.push(ask("earnings", t("bot.greeting.ask.earnings.label", { name: next.name }), t("bot.greeting.ask.earnings.hint"), t("bot.greeting.ask.earnings.prompt", { name: next.name })));
      if (followed.length > 1) cards.push(ask("movers", t("bot.greeting.ask.movers.label"), t("bot.greeting.ask.movers.hint"), t("bot.greeting.ask.movers.prompt")));
    }
  }
  const shownCards = cards.slice(0, MAX_ASKS);
  // A new user's cards are mostly first steps (follow, briefing, account), not questions.
  const asksLabel = followed.length ? t("bot.greeting.asks") : t("bot.greeting.start");
  // The principle step stays a quiet line once the user follows something, until it is done or hidden.
  const offerPrinciple = Boolean(read && followed.length && read.principles === 0 && !principleOff);
  const greeting = t(greetingKey(new Date(opened.nowMs).getHours()));
  const today = formatDate(opened.nowMs, { year: undefined, month: "long", day: "numeric", weekday: "long" });

  return (
    <section className="bot-greeting" data-bot-greeting="" aria-label={t("bot.greeting.label", { name: loupist.name })} aria-busy={!read}>
      <header className="bot-greeting-top">
        <LoupistAvatar loupist={loupist} size={56} />
        <span>
          <strong>{greeting}</strong>
          {/* The page header above already names the Loupist. */}
          <small>{today}</small>
        </span>
      </header>
      {read ? <>
        <div className="bot-greeting-bubbles">
          {!followed.length ? <p className="bot-bubble">{t("bot.greeting.introNew")}</p> : null}
          <p className="bot-bubble">{t("bot.greeting.prompt")}</p>
        </div>
        {shownCards.length ? <section className="bot-greeting-group" aria-label={asksLabel}>
          <p className="bot-greeting-label" aria-hidden="true">{asksLabel}</p>
          <ul ref={asksRef} className="bot-asks">
            {shownCards.map((card) => <li key={card.key}>
              <button type="button" className="start-option-card bot-ask" data-ask={card.key} onClick={card.run}>
                <span><strong>{card.label}</strong>{card.hint ? <small>{card.hint}</small> : null}</span>
                {/* Not Send's arrow: a card fills the composer or opens a page, as the news's questions do. */}
                <ArrowRightIcon aria-hidden="true" />
              </button>
            </li>)}
          </ul>
          {offerPrinciple ? <p className="bot-greeting-suggestion">
            <button type="button" className="bot-greeting-suggestion-text" data-ask="principle" onClick={() => onAsk(t("today.setup.principle.insert"))}>{t("bot.greeting.setup.principle", { name: loupist.name })}</button>
            <button type="button" className="bot-greeting-suggestion-hide" aria-label={t("bot.greeting.setup.hide")} title={t("bot.greeting.setup.hide")}
              onClick={() => {
                hidePrinciple(loupistId);
                setPrincipleOff(true);
                [...(asksRef.current?.querySelectorAll("button") ?? [])].at(-1)?.focus({ preventScroll: true });
              }}><XIcon aria-hidden="true" /></button>
          </p> : null}
        </section> : null}
        {priced.length ? <section className="bot-greeting-group" aria-label={t("bot.greeting.following")}>
          <p className="bot-greeting-label" aria-hidden="true">{t("bot.greeting.following")}</p>
          <ul className="bot-tiles">
            {priced.map((item) => {
              const { price } = item.quote!;
              // The price as the stored replay states it: value, currency, date and provider; never a session word.
              const provider = providerName(price.provider);
              // The name says where the tile goes; the price and its date are read after it.
              const priceId = `${tileIds}-${item.key}-price`;
              const footId = `${tileIds}-${item.key}-foot`;
              return <li key={item.key}>
                <button type="button" className="bot-tile" aria-label={t("bot.greeting.openAsset", { name: item.name })} aria-describedby={`${priceId} ${footId}`} disabled={!go}
                  onClick={() => go?.({ kind: "asset", target: { loupistId, assetId: item.key, name: item.name } })}>
                  <span className="bot-tile-name">{item.name}</span>
                  {/* Written as the asset page writes it: every stated digit of a price without a currency. */}
                  <span id={priceId} className="bot-tile-price-line"><PriceText value={price.value} currency={price.currency?.trim() || undefined} digits="auto" size="body" className="bot-tile-price" />
                    {/* As the asset page says it: a price whose currency nothing states is marked, never left bare. */}
                    {price.currency?.trim() ? null : <> <span className="bot-tile-currency" data-price-currency="unknown">{i18n.t("assets:price.currencyUnknown")}</span></>}</span>
                  <span id={footId} className="bot-tile-foot" data-price-date={price.date}>{joinMeta([priceAsOf(price.date), provider])}</span>
                </button>
              </li>;
            })}
          </ul>
        </section> : null}
      </> : null}
    </section>
  );
}
