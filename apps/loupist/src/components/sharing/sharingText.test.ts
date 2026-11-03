import { shortTitle } from "./sharingText";

it("quotes a short title as written, with whitespace collapsed", () => {
  expect(shortTitle("Owner earnings")).toBe("Owner earnings");
  expect(shortTitle("  Trim when\n a position  passes 12% ")).toBe("Trim when a position passes 12%");
});

it("cuts a long title at a word boundary with an ellipsis", () => {
  expect(shortTitle("Trim when a position passes 12% of the account, then rebalance into cash", 40)).toBe("Trim when a position passes 12% of the…");
  expect(shortTitle("Trim when a position passes 12%, then rebalance", 33)).toBe("Trim when a position passes 12%…");
});

it("cuts a title without spaces at the limit and counts characters, not UTF-16 units", () => {
  expect(shortTitle("포지션이계좌의12퍼센트를넘으면줄인다", 10)).toBe("포지션이계좌의12퍼…");
  expect(shortTitle("📈".repeat(12), 10)).toBe(`${"📈".repeat(10)}…`);
});
