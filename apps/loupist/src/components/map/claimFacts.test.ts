import type { TFunction } from "i18next";
import i18n from "../../i18n";
import type { GraphClaim, TimePoint } from "../../contracts/knowledge";
import { dateValue } from "../../knowledge/presentation";
import { claimCorrectionDraft, claimWords, orderEntityClaims, validityLine } from "./claimFacts";

const t = i18n.getFixedT(null, "knowledge") as TFunction<"knowledge">;
const NOW = Date.parse("2026-06-01T00:00:00Z");
const point = (value: string, approximate = false): TimePoint => ({ precision: "date", value, approximate });
const claim = (status: GraphClaim["status"], validAt: TimePoint | null, invalidAt: TimePoint | null, recordedAtMs = 1): GraphClaim => ({
  id: `${status}:${validAt?.value ?? ""}:${invalidAt?.value ?? ""}:${recordedAtMs}`,
  proposition: "Margin was 14%", attribution: "", relation: "REPORTS", endpoints: [{ nodeId: "n", role: "subject" }],
  qualifiers: [], evidence: [], status, recordedAtMs, publicationTime: point("2024-03-31"), effectiveTime: null, validAt, invalidAt, expiredAt: null,
});

it.each([
  [claim("active", point("2020-01-01"), null), "current", "Held since"],
  [claim("active", point("2020-01-01"), point("2024-12-31")), "ended", "Held"],
  [claim("active", point("2020-01-01"), point("2024-12-31", true)), "ended", "ended no later than"],
  [claim("active", point("2020-01-01"), point("2027-01-01")), "current", "Ends"],
  [claim("active", null, null), "current", "Start not stated"],
  [claim("disputed", point("2020-01-01"), point("2024-01-01")), "disputed", "Held"],
  [claim("withdrawn", point("2020-01-01"), null), "withdrawn", "Held since"],
] as const)("words a claim's status and validity", (item, words, validity) => {
  expect(claimWords(item, NOW)).toBe(words);
  const line = validityLine(item, t, NOW);
  expect(line).toContain(validity);
  if (item.invalidAt?.approximate) expect(line).toBe(`Held since ${dateValue(item.validAt!.value!)} · ended no later than ${dateValue(item.invalidAt.value!)}`);
});

it("lists current claims before ended ones", () => {
  const current = claim("active", point("2020-01-01"), null, 2);
  const ended = claim("active", point("2020-01-01"), point("2024-01-01"), 9);
  const disputed = claim("disputed", point("2020-01-01"), point("2024-01-01"), 3);
  const withdrawn = claim("withdrawn", point("2020-01-01"), null, 8);
  const edge = (item: GraphClaim) => ({ endpoints: item.endpoints, claim: item });
  expect(orderEntityClaims([edge(ended), edge(withdrawn), edge(current), edge(disputed)], "n", NOW).map(item => item.claim.status + (item.claim.invalidAt && item.claim.status === "active" ? ":ended" : ""))).toEqual([
    "disputed", "active", "active:ended", "withdrawn",
  ]);
});

it("quotes the claim, its dates and each Source title once, and ends where the user's reason starts (plan 4.6)", () => {
  const wrong: GraphClaim = {
    ...claim("disputed", point("2025-04-01"), null),
    proposition: "Ventas netas de 2025: 4,2 mil millones de MXN",
    publicationTime: point("2026-02-20"),
    evidence: [
      { spanId: "a", role: "supports", attribution: "Informe anual 2025" },
      { spanId: "b", role: "supports", attribution: "Informe anual 2025" },
      { spanId: "c", role: "opposes", attribution: "決算短信 2025" },
    ],
  };
  const draft = claimCorrectionDraft(wrong, t, NOW);
  expect(draft.split("\n")).toEqual([
    "About this fact in my World:",
    "> Ventas netas de 2025: 4,2 mil millones de MXN",
    `> ${validityLine(wrong, t, NOW)} · Published ${dateValue("2026-02-20")}`,
    "> Sources: Informe anual 2025; 決算短信 2025",
    "",
    "This is wrong because ",
  ]);
  // A claim with no bound evidence still names the Source it was attributed to.
  expect(claimCorrectionDraft({ ...wrong, evidence: [], attribution: "Rapport annuel" }, t, NOW)).toContain("> Sources: Rapport annuel");
});
