import { useState } from "react";
import { formatDateTime, formatDay } from "@/i18n/format";
import { useTranslation } from "@/i18n";
import type { ChatMessage, LoupistBridge } from "../../types";
import type { HomeAsk, HomeOffer } from "../../contracts/chat";
import { Button } from "../ui/button";
import { BotPostBody } from "./BotPost";
import "@/styles/bot-cards.css";

function presetLabel(preset: HomeOffer["preset"]) {
  switch (preset) {
    case "filings-and-results": return "schedule.filings" as const;
    case "thesis-lines": return "schedule.lines" as const;
    case "results-follow-up": return "schedule.results" as const;
    case "week-in-review": return "schedule.week" as const;
    case "price-move": return "schedule.priceMoves" as const;
  }
}

export function ThreadCards({ message, bridge, loupistId, loupistName, host }: {
  message: ChatMessage;
  bridge?: LoupistBridge;
  loupistId: string;
  loupistName: string;
  /** The message is only the card, so it carries the Loupist's face. */
  host?: boolean;
}) {
  const cards = <>
    {message.offers?.map(offer => <OfferCard key={offer.id} offer={offer} bridge={bridge} loupistId={loupistId} />)}
    {message.ask ? <AskCard ask={message.ask} bridge={bridge} loupistId={loupistId} /> : null}
  </>;
  if (!host) return cards;
  return <BotPostBody loupistName={loupistName} at={Date.parse(message.created_at)}>{cards}</BotPostBody>;
}

function OfferCard({ offer, bridge, loupistId }: { offer: HomeOffer; bridge?: LoupistBridge; loupistId: string }) {
  const { t } = useTranslation("briefings");
  const { t: chatT } = useTranslation("chat");
  const [state, setState] = useState(offer.taskId ? "on" : offer.dismissed ? "dismissed" : "open");
  const [error, setError] = useState(false);
  const expired = state === "open" && Date.now() >= Math.min(offer.expiresAtMs, offer.runAtMs ?? Infinity);
  const act = async (action: "accept" | "dismiss") => {
    if (!bridge || state !== "open") return;
    setError(false);
    try {
      if (action === "accept") await bridge.botOfferAccept({ loupistId, offerId: offer.id });
      else await bridge.botOfferDismiss({ loupistId, offerId: offer.id });
      setState(action === "accept" ? "on" : "dismissed");
    } catch {
      setError(true);
    }
  };
  return <section className="bot-card" data-offer={offer.id} aria-label={t("standing.offer")}>
    <div className="bot-card-kicker"><span className="bot-card-tag">{t("standing.offer")}</span></div>
    <h3>{t(presetLabel(offer.preset))} · {offer.assetName}</h3>
    <p>{offer.reason}</p>
    {offer.resultsDate && <p className="briefings-meta">
      <time dateTime={offer.resultsDate}>{formatDay(offer.resultsDate)}</time>
      {offer.runAtMs != null && <> · <span>{chatT("messageSchedules.nextRun", { date: formatDateTime(offer.runAtMs) })}</span></>}
    </p>}
    {expired ? <p className="briefings-meta">{t("standing.expired")}</p> : state === "on" ? <p className="briefings-meta">{t("schedule.turnOn")}</p> : state === "open" ? <div className="thread-alert-actions">
      <Button size="sm" onClick={() => void act("accept")}>{t("schedule.turnOn")}</Button>
      <Button size="sm" variant="ghost" onClick={() => void act("dismiss")}>{t("standing.notNow")}</Button>
    </div> : null}
    {error ? <p className="inline-error" role="alert">{t("schedule.statusFailed")}</p> : null}
  </section>;
}

function AskCard({ ask, bridge, loupistId }: { ask: HomeAsk; bridge?: LoupistBridge; loupistId: string }) {
  const { t } = useTranslation("briefings");
  const [choice, setChoice] = useState<number | null>(ask.choice ?? null);
  const [error, setError] = useState(false);
  const expired = choice === null && Date.now() >= ask.expiresAtMs;
  const pick = async (index: number) => {
    if (!bridge || choice !== null) return;
    setError(false);
    try {
      await bridge.botAskAnswer({ loupistId, askId: ask.id, choice: index });
      setChoice(index);
    } catch {
      setError(true);
    }
  };
  return <section className="bot-card" data-ask={ask.id} aria-label={t("standing.ask")}>
    <div className="bot-card-kicker"><span className="bot-card-tag">{t("standing.ask")}</span></div>
    <h3>{ask.question}</h3>
    {expired ? <p className="briefings-meta">{t("standing.expired")}</p> : <div className="thread-alert-actions">
      {ask.options.map((option, index) => <Button key={option.label} size="sm" variant={choice === index || (choice === null && index === ask.recommended) ? "default" : "outline"} disabled={choice !== null} onClick={() => void pick(index)}>
        {option.label}{index === ask.recommended ? ` · ${t("standing.recommended")}` : ""}
      </Button>)}
    </div>}
    {choice !== null ? <p className="briefings-meta">{t("standing.chosen", { choice: ask.options[choice]?.label ?? "" })}</p> : null}
    {error ? <p className="inline-error" role="alert">{t("schedule.statusFailed")}</p> : null}
  </section>;
}
