import { delimitedPreview, PREVIEW_COLUMNS, PREVIEW_ROWS } from "./delimitedPreview";

it("preserves quoted separators, newlines, escaped quotes, whitespace and exact financial text", () => {
  const result = delimitedPreview('\uFEFFsymbol,company,value,note\r\n005930,"Samsung, Inc.",000123.4500,"line one\r\nline ""two"""\r\nAAPL, Apple ,=1+1,');
  expect(result.error).toBeNull();
  expect(result.rows).toEqual([
    ["symbol", "company", "value", "note"],
    ["005930", "Samsung, Inc.", "000123.4500", 'line one\r\nline "two"'],
    ["AAPL", " Apple ", "=1+1", ""],
  ]);
  expect(result.rowCount).toBe(3);
  expect(result.columnCount).toBe(4);
});

it("parses TSV, CR line endings, blank records and a final unterminated record", () => {
  expect(delimitedPreview('a\tb\rc\t"d\te"', "\t").rows).toEqual([["a", "b"], ["c", "d\te"]]);
  expect(delimitedPreview("").rows).toEqual([]);
  expect(delimitedPreview("a\n\nb\n").rows).toEqual([["a"], [""], ["b"]]);
});

it("bounds stored rows and columns while counting all complete records", () => {
  const row = Array.from({ length: PREVIEW_COLUMNS + 3 }, (_, index) => `c${index}`).join(",");
  const result = delimitedPreview(Array.from({ length: PREVIEW_ROWS + 4 }, () => row).join("\n"));
  expect(result.rowCount).toBe(PREVIEW_ROWS + 4);
  expect(result.columnCount).toBe(PREVIEW_COLUMNS + 3);
  expect(result.rows).toHaveLength(PREVIEW_ROWS + 1);
  expect(result.rows.every((value) => value.length === PREVIEW_COLUMNS)).toBe(true);
});

it("does not present incomplete file-preview tails as real data", () => {
  for (const tail of ['AAPL,"partial', 'AAPL,12']) {
    const result = delimitedPreview(`symbol,value\n${tail}`, ",", true);
    expect(result.rows).toEqual([["symbol", "value"]]);
    expect(result.incompleteLastRow).toBe(true);
    expect(result.error).toBeNull();
  }
  expect(delimitedPreview("a,b\n", ",", true).incompleteLastRow).toBe(false);
});

it("reports malformed quoting instead of silently assigning shifted values", () => {
  for (const source of ['a,"unfinished', 'a,un"quoted', 'a,"quoted"tail']) {
    expect(delimitedPreview(source).error).toMatch(/quote/);
  }
});
