import i18n, { type resources } from "@/i18n";
import type { AssetPricePoint, AssetReferenceFields, AssetStoredPrice } from "../../contracts/assets";
import type { TaskSubject } from "../../contracts/generated/scheduled_tasks/types";
import type { ScheduledTaskListItem } from "../../types";
import { listingExchange } from "../assets/listingExchange";

type SecurityType = keyof typeof resources.en.briefings.following.securityType;
const types = new Map<string, SecurityType>([
  ["common stock", "commonStock"],
  ["preferred stock", "preferredStock"], ["preference", "preferredStock"],
  ["etf", "etf"], ["exchange traded fund", "etf"], ["exchange traded funds", "etf"],
  ["etp", "etp"], ["etf/etp", "etp"], ["exchange traded product", "etp"],
  ["etn", "etn"],
  ["adr", "americanDepositaryReceipt"], ["gdr", "globalDepositaryReceipt"],
  ["depositary receipt", "depositaryReceipt"], ["depositary receipts", "depositaryReceipt"],
  ["canadian dr", "canadianDepositaryReceipt"], ["cedear", "cedear"],
  ["reit", "reit"], ["fund", "fund"], ["mutual fund", "mutualFund"],
  ["index", "index"], ["equity index", "equityIndex"],
  ["bond", "bond"], ["bond/note", "bond"], ["note", "note"],
  ["priv placement", "privatePlacement"],
  ["warrant", "warrant"], ["right", "right"], ["unit", "unit"],
  ["unit investment trust", "unitInvestmentTrust"], ["partnership shares", "partnershipShares"],
  ["future", "future"], ["single stock future", "singleStockFuture"],
  ["option", "option"], ["equity option", "equityOption"],
  ["us domestic", "usDomesticBond"], ["canadian", "canadianBond"],
]);

/** Format generic listing labels with the interface language, keeping the provider's reference fields intact. */
export function listingLabel(language: string) {
  const t = i18n.getFixedT(language, "briefings");
  return (fields: AssetReferenceFields) => {
    const { exchange } = listingExchange(fields, language);
    const type = fields.securityType?.trim();
    const key = type && types.get(type.toLowerCase());
    return [fields.ticker?.trim(), exchange, key ? t(`following.securityType.${key}`) : type].filter(Boolean).join(" · ") || null;
  };
}

const subjectAsset = (subject: TaskSubject) => "asset" in subject ? subject.asset : null;

function taskCovers(task: Pick<ScheduledTaskListItem, "subjects" | "trigger">, assetId: string) {
  if (task.subjects?.some(subject => subjectAsset(subject) === assetId)) return true;
  const ids = task.trigger?.args.asset_ids;
  return Array.isArray(ids) && ids.includes(assetId);
}

export type FollowingBadge = { kind: "check" | "watch" | "daily"; count: number; failing: boolean };

/** Active checks, watches and a daily learning task for one followed asset. */
export function followingBadges(assetId: string, tasks: readonly ScheduledTaskListItem[]): FollowingBadge[] {
  const active = tasks.filter(task => task.status === "active");
  const checks = active.filter(task => Boolean(task.trigger) && taskCovers(task, assetId));
  const watches = active.filter(task => {
    const watched = Boolean(task.preset) && taskCovers(task, assetId);
    const custom = task.kind !== "learning" && !task.trigger && task.subjects?.some(subject => subjectAsset(subject) === assetId);
    return watched || custom;
  });
  const daily = active.filter(task => task.kind === "learning"
    && task.subjects?.length === 1 && subjectAsset(task.subjects[0]!) === assetId);
  const badges: FollowingBadge[] = [];
  if (checks.length) badges.push({ kind: "check", count: checks.length, failing: checks.some(task => task.trigger?.state === "failing") });
  if (watches.length) badges.push({ kind: "watch", count: watches.length, failing: false });
  if (daily.length) badges.push({ kind: "daily", count: 1, failing: false });
  return badges;
}

/** Active learning programs whose subject is this asset. Track daily is one of them. */
export function learningProgramsFor(assetId: string, tasks: readonly ScheduledTaskListItem[]) {
  return tasks.filter(task => task.status === "active" && task.kind === "learning" && task.subjects?.some(subject => subjectAsset(subject) === assetId));
}

/** The move from the latest stored point dated before this price. No earlier point is omitted, never zero. */
export function priceChange(price: Pick<AssetStoredPrice, "value" | "date">, points: readonly AssetPricePoint[]) {
  const previous = points.filter(point => point.date < price.date).sort((left, right) => left.date < right.date ? -1 : left.date > right.date ? 1 : 0).at(-1);
  if (!previous) return null;
  const delta = price.value - previous.value;
  const percent = previous.value === 0 ? null : delta / previous.value;
  return { delta, percent, date: previous.date };
}
