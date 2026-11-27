import type { TFunction } from "i18next";
import i18n from "../../i18n";
import { weeklyNameCount, worldStatusLine } from "./stats";

const t = i18n.getFixedT(null, "knowledge") as TFunction<"knowledge">
const NOW = Date.parse("2026-06-15T00:00:00Z");
const day = 86_400_000;

it("names each counter and leaves the weekly count out of a bounded World", () => {
  expect(worldStatusLine(t, { shownNames: 1375, totalNames: 1375, bounded: false, links: 1588, disagreements: 3, weekly: 12 }))
    .toBe("1,375 names · 1,588 relationships · 12 new names this week · 3 disagreements");
  expect(worldStatusLine(t, { shownNames: 991, totalNames: 1375, bounded: true, links: 1588, disagreements: 3, weekly: 12 }))
    .toBe("Showing 991 of 1,375 names · 1,588 relationships · 3 disagreements");
  expect(worldStatusLine(t, { shownNames: 1, totalNames: 1, bounded: false, links: 0, disagreements: 0, weekly: 0 })).toBe("1 name · 0 relationships");
});

it("counts projected entities first seen this week and never more than the total", () => {
  const entities = [
    { firstSeenAtMs: NOW - day },
    { firstSeenAtMs: NOW - 8 * day },
    { firstSeenAtMs: NOW + day },
    {},
  ];
  expect(weeklyNameCount(entities, NOW, 100)).toBe(1);
  expect(weeklyNameCount(entities, NOW, 0)).toBe(0);
});
