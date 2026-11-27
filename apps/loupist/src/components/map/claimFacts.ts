import type { TFunction } from "i18next";
import type { GraphClaim, TimePoint } from "../../contracts/knowledge";
import { dateValue } from "../../knowledge/presentation";

export type ClaimWords = "current" | "ended" | "disputed" | "withdrawn";

function instant(point: TimePoint | null | undefined) {
  const value = point?.value;
  if (!value) return undefined;
  if (/^\d{4}$/.test(value)) return Date.parse(`${value}-01-01T00:00:00Z`);
  if (/^\d{4}-\d{2}$/.test(value)) return Date.parse(`${value}-01T00:00:00Z`);
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? undefined : parsed;
}

/** A fact whose end is already known. A future end, exact or approximate, is still current. */
export function validityEnded(claim: Pick<GraphClaim, "invalidAt">, now: number) {
  const end = instant(claim.invalidAt);
  return end !== undefined && end <= now;
}

/** Disputed wins over an end date. Withdrawn is its own status. */
export function claimWords(claim: Pick<GraphClaim, "status" | "invalidAt">, now: number): ClaimWords {
  if (claim.status === "withdrawn") return "withdrawn";
  if (claim.status === "disputed") return "disputed";
  if (validityEnded(claim, now)) return "ended";
  return "current";
}

const shown = (point: TimePoint | null | undefined) => point?.value ? dateValue(point.value) : undefined;

/** The validity line. An approximate end says it ended no later than that date. */
export function validityLine(claim: Pick<GraphClaim, "validAt" | "invalidAt">, t: TFunction<"knowledge">, now: number) {
  const start = shown(claim.validAt);
  const end = shown(claim.invalidAt);
  const future = claim.invalidAt?.value != null && !validityEnded(claim, now);
  if (claim.invalidAt?.approximate && end && !future) {
    return start ? t("map.validity.approximate", { start, end }) : t("map.validity.approximateNoStart", { end });
  }
  if (future && end) return t("map.validity.ends", { end });
  if (start && end) return t("map.validity.range", { start, end });
  if (start) return t("map.validity.since", { start });
  return t("map.validity.noStart");
}

export function publishedLine(claim: Pick<GraphClaim, "publicationTime">, t: TFunction<"knowledge">) {
  const date = shown(claim.publicationTime);
  return date ? t("map.published", { date }) : undefined;
}

export function learnedLine(claim: Pick<GraphClaim, "recordedAtMs">, t: TFunction<"knowledge">) {
  if (claim.recordedAtMs == null) return undefined;
  return t("map.learned", { date: dateValue(new Date(claim.recordedAtMs).toISOString()) });
}

/** Current claims first (a dispute stays with them), then ended, then withdrawn. */
export function orderEntityClaims<T extends { endpoints: { nodeId: string }[]; claim: GraphClaim }>(edges: readonly T[], nodeId: string, now: number) {
  const rank = (claim: GraphClaim) => claim.status === "withdrawn" ? 2 : validityEnded(claim, now) && claim.status !== "disputed" ? 1 : 0;
  return edges.filter(edge => edge.endpoints.some(endpoint => endpoint.nodeId === nodeId))
    .sort((a, b) => rank(a.claim) - rank(b.claim) || (b.claim.recordedAtMs ?? 0) - (a.claim.recordedAtMs ?? 0));
}

/**
 * The composer text of "Tell Loupist this is wrong" (plan 4.6): the claim's
 * proposition, its dates and its Source titles quoted, then the start of the
 * user's own sentence, which the caret follows. It is an ordinary message;
 * nothing is sent until the user sends it.
 */
export function claimCorrectionDraft(claim: Pick<GraphClaim, "proposition" | "validAt" | "invalidAt" | "publicationTime" | "evidence" | "attribution">, t: TFunction<"knowledge">, now: number) {
  const dates = [validityLine(claim, t, now), publishedLine(claim, t)].filter(Boolean).join(" · ");
  const titles = [...new Set([...claim.evidence.map(binding => binding.attribution), claim.evidence.length ? "" : claim.attribution].map(title => title.trim()).filter(Boolean))];
  const quoted = [claim.proposition.trim(), dates, titles.length ? t("map.correction.sources", { titles: titles.join("; ") }) : ""]
    .filter(Boolean).flatMap(line => line.split(/\r?\n/u)).map(line => `> ${line}`);
  return [t("map.correction.intro"), ...quoted, "", t("map.correction.because")].join("\n");
}
