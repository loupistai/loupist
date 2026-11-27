import { columnListSummary, sourcePreviewLine } from "./sourcePreview";

it.each([
  ["Economy · Period · Population, total", true],
  ["The filing reports segment sales for the Americas and Europe.", false],
  ["Income statement", false],
  [undefined, false],
])("treats a middle-dot column list as structured (%j)", (summary, structured) => {
  expect(columnListSummary(summary)).toBe(structured);
});

it("previews a structured Source as what it is, with rows and period when the catalog has them", () => {
  const columns = "Economy · Period · Population, total";
  expect(sourcePreviewLine({ summary: columns, originLabel: "Income statement" })).toBe("Income statement");
  expect(sourcePreviewLine({
    summary: columns,
    originLabel: "FRED",
    table: { statement: "Income statement", rows: 12, period: "2023 Q4–2026 Q3" },
  })).toBe("Income statement · 12 rows · 2023 Q4–2026 Q3");
  expect(sourcePreviewLine({ summary: "The filing reports segment sales." })).toBe("The filing reports segment sales.");
});
