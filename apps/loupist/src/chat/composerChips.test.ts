import { describe, expect, it } from "vitest";
import { composeSubmission, rankText } from "./composerChips";

describe("composer chips", () => {
  it("ranks names ahead of incidental matches", () => {
    expect(rankText("fil", ["Filings", "Read a filing"])).toBeGreaterThan(rankText("fil", ["A profile note"]));
    expect(rankText("", ["Anything"])).toBe(1);
    expect(rankText("zzz", ["Filings"])).toBe(0);
  });

  it("serializes chips into the existing skill, plugin, and file text", () => {
    expect(composeSubmission("Compare margins", [
      { key: "skill:research", kind: "skill", id: "research", label: "Research" },
      { key: "connection:opendart.disclosures", kind: "connection", id: "opendart.disclosures", label: "OpenDART" },
      { key: "file:reports/note.md", kind: "file", id: "reports/note.md", label: "note.md" },
    ])).toBe("$research @opendart.disclosures Look at workspace file reports/note.md.\n\nCompare margins");
  });

  it("does not repeat a token the user already typed", () => {
    expect(composeSubmission("Use $research", [
      { key: "skill:research", kind: "skill", id: "research", label: "Research" },
    ])).toBe("Use $research");
  });
});
