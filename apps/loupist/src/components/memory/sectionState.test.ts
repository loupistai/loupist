import { dateValue } from "../../knowledge/presentation";
import { changedSectionCount, sectionRoleLabel, sectionState, supersededChipLabel } from "./sectionState";

const section = { contested: false };

it.each([
  [{ ...section, contested: true, sinceMs: 1, claimIds: ["old"] }, "contested"],
  [{ ...section, sinceMs: 1, claimIds: ["old"] }, "superseded"],
  [{ ...section, sinceMs: 1, claimIds: [] }, "current"],
  [section, "current"],
])("a contested section wins over superseded (%j)", (facts: { contested: boolean; sinceMs?: number; claimIds?: string[] }, state: string) => {
  expect(sectionState(facts)).toBe(state);
});

it("counts sections that rest on a fact that changed", () => {
  expect(changedSectionCount([
    { contested: true },
    { contested: false, sinceMs: 1, claimIds: ["old"] },
    { contested: false, role: "check" },
  ])).toBe(2);
});

it("labels a check, an open question and a refutation, and nothing else", () => {
  expect(sectionRoleLabel("check")).toBe("Check");
  expect(sectionRoleLabel("open_question")).toBe("Open question");
  expect(sectionRoleLabel("refutation")).toBe("Would break it");
  expect(sectionRoleLabel("fact")).toBeUndefined();
  expect(sectionRoleLabel("view")).toBeUndefined();
  expect(sectionRoleLabel(undefined)).toBeUndefined();
});

it("builds the superseded chip and shortens the Source before the date", () => {
  expect(supersededChipLabel({})).toBe("Superseded");
  expect(supersededChipLabel({ end: "2024-03-31" })).toBe("Superseded");
  const date = dateValue("2024-03-31");
  expect(supersededChipLabel({ end: "2024-03-31", source: "Q3 filing" })).toBe(`Superseded · ${date} · Q3 filing`);
  const source = "Quarterly report on the supplier's capacity and the margin that followed";
  const label = supersededChipLabel({ end: "2024-03-31", source });
  expect(label.startsWith(`Superseded · ${date} · `)).toBe(true);
  expect(label.endsWith("…")).toBe(true);
  expect([...label].length).toBeLessThanOrEqual(64);
  expect(label.includes(date)).toBe(true);
});
