import { useEffect, useId, useState, type ReactNode } from "react";
import { ExternalLinkIcon } from "lucide-react";
import { useTranslation } from "../../i18n";
import { formatDay, formatNumber, formatPercent, formatTime } from "../../i18n/format";
import { providerShortName } from "../../chat/providerNames";
import type { AllowanceState, OpenRouterQuota, UsageSummary, UsageSummaryRequest, UsageTotals } from "../../contracts/generated";
import type { LoupistBridge, ModelCatalog } from "../../types";
import { Button } from "../ui/button";
import { allowanceNamesItself, backgroundLine, creditLine, holdLine, sourcesLine, worldShareLine } from "./allowanceCopy";

type Period = 1 | 7 | 30;

function usageRange(period: Period, now = Date.now()): UsageSummaryRequest {
  const start = new Date(now);
  start.setDate(start.getDate() - (period - 1));
  start.setHours(0, 0, 0, 0);
  return {
    fromMs: start.getTime(),
    toMs: now + 1,
    timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC",
  };
}

export function useUsageSummary(
  bridge: Pick<LoupistBridge, "usageSummary">,
  period: Period,
  enabled: boolean,
) {
  const [result, setResult] = useState<{ period: Period; summary: UsageSummary | null }>();
  useEffect(() => {
    if (!enabled) return;
    let current = true;
    void bridge.usageSummary(usageRange(period)).then(
      summary => { if (current) setResult({ period, summary }); },
      () => { if (current) setResult({ period, summary: null }); },
    );
    return () => { current = false; };
  }, [bridge, period, enabled]);
  return result?.period === period ? result.summary : undefined;
}

function UsageGroup({ title, children }: { title: string; children: ReactNode }) {
  const id = useId();
  return <section className="usage-group" aria-labelledby={id}>
    <h2 id={id}>{title}</h2>
    {children}
  </section>;
}

function requestCount(totals: UsageTotals) {
  return { count: totals.requests, formattedCount: formatNumber(totals.requests) };
}

function DailyRequests({ days }: { days: UsageSummary["days"] }) {
  const { t } = useTranslation("settings");
  const categories = ["you", "background", "world"] as const;
  const maximum = Math.max(1, ...days.map(day => day.you + day.background + day.world));
  return <table className="usage-days">
    <thead><tr>
      <th scope="col">{t("usage.date")}</th>
      <td aria-hidden="true" />
      {categories.map(category => <th scope="col" key={category}>
        <span className="usage-swatch" data-category={category} aria-hidden="true" />
        {t(`usage.${category}`)}
      </th>)}
    </tr></thead>
    <tbody>{days.map(day => <tr key={day.date}>
      <th scope="row"><time dateTime={day.date}>{formatDay(day.date)}</time></th>
      <td aria-hidden="true" className="usage-bar-cell"><div className="usage-bar">
        {categories.map(category => <span key={category} data-category={category} style={{ flexGrow: day[category] }} />)}
        <span style={{ flexGrow: maximum - day.you - day.background - day.world }} />
      </div></td>
      {categories.map(category => <td key={category}>{formatNumber(day[category])}</td>)}
    </tr>)}</tbody>
  </table>;
}

type Props = {
  bridge: Pick<LoupistBridge, "usageSummary" | "providerOpenUsage"> & Partial<Pick<LoupistBridge, "providerQuotaRead">>;
  catalog: ModelCatalog | null;
  active?: boolean;
  onOpenProgram?: (loupistId: string, taskId: string) => void;
};

function AllowanceLines({ allowance, credit }: { allowance: AllowanceState; credit: OpenRouterQuota | null }) {
  const { t } = useTranslation("settings");
  const backgroundNamed = allowanceNamesItself(allowance.background.state);
  const sourcesNamed = allowanceNamesItself(allowance.worldChat.state);
  return <UsageGroup title={t("usage.allowance.title")}>
    <ul className="usage-list">
      <li className="usage-row" data-allowance="background" data-state={allowance.background.state}>
        {backgroundNamed ? <span>{backgroundLine(allowance.background)}</span> : <><span>{t("usage.allowance.background")}</span><span className="usage-value">{backgroundLine(allowance.background)}</span></>}
      </li>
      <li className="usage-row" data-allowance="sources" data-state={allowance.worldChat.state}>
        {sourcesNamed ? <span>{sourcesLine(allowance.worldChat)}</span> : <><span>{t("usage.allowance.sources")}</span><span className="usage-value">{sourcesLine(allowance.worldChat)}</span></>}
      </li>
      {allowance.background.state === "off" ? null : <li className="usage-row" data-allowance="world"><span>{worldShareLine(allowance.worldShare)}</span></li>}
      {Object.entries(allowance.providerHolds).map(([key, hold]) => <li key={key} className="usage-row" data-allowance-hold={hold.reason}><span>{holdLine(hold)}</span></li>)}
      {credit ? <li className="usage-row" data-allowance="credit"><span>{creditLine(credit)}</span></li> : null}
    </ul>
  </UsageGroup>;
}

export function UsageSettings({ bridge, catalog, active = true, onOpenProgram }: Props) {
  const { t } = useTranslation("settings");
  const [period, setPeriod] = useState<Period>(7);
  const [manageError, setManageError] = useState(false);
  const [credit, setCredit] = useState<OpenRouterQuota | null>(null);
  const summary = useUsageSummary(bridge, period, active);
  const readQuota = bridge.providerQuotaRead;
  useEffect(() => {
    if (!active || !readQuota) { setCredit(null); return; }
    let live = true;
    void readQuota("openrouter").then(value => { if (live) setCredit(value); }, () => { if (live) setCredit(null); });
    return () => { live = false; };
  }, [active, readQuota]);

  return <div className="usage-settings">
    <div className="usage-period" role="group" aria-label={t("usage.period")}>
      {([1, 7, 30] as const).map(days => <Button type="button" key={days}
        variant={period === days ? "secondary" : "ghost"} size="sm" aria-pressed={period === days}
        onClick={() => { setPeriod(days); setManageError(false); }}>
        {t(days === 1 ? "usage.today" : "usage.days", { days })}
      </Button>)}
    </div>
    {active && summary === undefined ? <p role="status">{t("usage.loading")}</p> : null}
    {summary === null ? <p role="alert">{t("usage.loadError")}</p> : null}
    {summary ? <AllowanceLines allowance={summary.allowance} credit={credit} /> : null}
    {manageError ? <p role="alert">{t("usage.manageError")}</p> : null}
    {summary && summary.totals.runs === 0 && summary.programs.length === 0 ? <p role="status">{t("usage.empty")}</p> : null}
    {summary && summary.totals.runs > 0 ? <>
      <p role="status" className="usage-total">{t("usage.requests", requestCount(summary.totals))}</p>
      {summary.totals.unknownRuns > 0 ? <p className="usage-muted">
        {t("usage.unknownRuns", { count: summary.totals.unknownRuns, formattedCount: formatNumber(summary.totals.unknownRuns) })}
      </p> : null}
      <UsageGroup title={t("usage.accounts")}>
        <ul className="usage-list">{summary.accounts.map(account => {
          const option = catalog?.providers.find(provider => provider.id === account.provider);
          const custom = catalog?.custom_providers.find(provider => provider.id === account.provider);
          const label = custom?.name ?? providerShortName(account.provider, account.credentialKind ?? undefined) ?? option?.label ?? account.provider;
          const input = account.totals.inputTokens + account.totals.cachedInputTokens;
          const kind = account.credentialKind;
          const cost = kind === "api_key" || kind === "environment" ? account.totals.costEstimate : null;
          const chatgpt = kind === "plan" && option?.billing_note === "chatgpt_plan";
          const grok = kind === "plan" && account.provider === "xai";
          const price = cost && formatNumber(cost.microusd / 1_000_000, {
            style: "currency", currency: "USD", currencyDisplay: "narrowSymbol",
          });
          return <li key={account.provider}>
            <div className="usage-row">
              <h3>{label}</h3>
              <p className="usage-value">{t("usage.requests", requestCount(account.totals))}</p>
            </div>
            <div className="usage-row usage-muted">
                <p>{t("usage.tokens", {
                  tokens: formatNumber(input),
                  cached: formatPercent(input ? account.totals.cachedInputTokens / input : 0),
                  output: formatNumber(account.totals.outputTokens),
                })}</p>
                {cost ? <p className="usage-value">{cost.microusd < 10_000
                  ? t("usage.costUnderCent")
                  : t(cost.fromProvider ? "usage.costBilled" : "usage.costEstimate", { price })}</p> : null}
            </div>
            {chatgpt || grok ? <div className="usage-plan">
              <p>{t("usage.plan", { account: label })}</p>
              {chatgpt ? <>
                <p>{t("usage.weeklyLimit")}</p>
                <button type="button" className="connect-link" onClick={() => {
                  setManageError(false);
                  void bridge.providerOpenUsage({ provider: account.provider }).catch(() => setManageError(true));
                }}>{t("providers.manageUsage")}<ExternalLinkIcon aria-hidden="true" /></button>
              </> : null}
            </div> : null}
          </li>;
        })}</ul>
      </UsageGroup>
      <UsageGroup title={t("usage.daily")}><DailyRequests days={summary.days} /></UsageGroup>
      <UsageGroup title={t("usage.work")}>
        <ul className="usage-list">{summary.surfaces.map(({ surface, totals }) => <li key={surface} className="usage-row">
          <span>{t(`usage.surfaces.${surface}`)}</span><span className="usage-value">{t("usage.requests", requestCount(totals))}</span>
        </li>)}</ul>
      </UsageGroup>
      <UsageGroup title={t("usage.loupists")}>
        <ul className="usage-list">{summary.loupists.map(({ loupistId, name, totals }) => <li key={loupistId ?? ""} className="usage-row">
          <span>{name ?? t("usage.removedLoupist")}</span><span className="usage-value">{t("usage.requests", requestCount(totals))}</span>
        </li>)}</ul>
      </UsageGroup>
    </> : null}
    {summary && summary.programs.length > 0 ? <UsageGroup title={t("usage.programs")}>
      <ul className="usage-list">{summary.programs.map(({ loupistId, taskId, title, runs, totals, checks }) => <li key={JSON.stringify([loupistId, taskId])} className={checks ? undefined : "usage-row"}>
        {title && onOpenProgram ? <button type="button" className="connect-link" onClick={() => onOpenProgram(loupistId, taskId)}>{title}</button>
          : <span>{title ?? t("usage.removedProgram")}</span>}
        <span className={checks ? "usage-muted usage-program-activity" : "usage-value"}>{checks ? [
          checks.lastCheckAtMs == null ? null : t("usage.lastCheck", { time: formatTime(checks.lastCheckAtMs) }),
          t(period === 1 ? "usage.checksToday" : "usage.checks", { count: checks.evaluations, formattedCount: formatNumber(checks.evaluations) }),
          t("usage.dataReads", { count: checks.reads, formattedCount: formatNumber(checks.reads) }),
          t("usage.met", { formattedCount: formatNumber(checks.met) }),
          t("usage.modelRequests", { formattedCount: formatNumber(totals.requests) }),
        ].filter(Boolean).join(" · ") : `${t("usage.runs", { count: runs, formattedCount: formatNumber(runs) })} · ${t("usage.requests", requestCount(totals))}`}</span>
      </li>)}</ul>
    </UsageGroup> : null}
  </div>;
}
