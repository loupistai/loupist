import { readableCitedPassages } from "./citedPassages";

it("maps UTF-8 boundaries to UTF-16 through Korean, Chinese, and a surrogate pair", () => {
  const text = "한글 中文😀 25원";
  expect(readableCitedPassages(text, [{ start: 7, end: 23, marks: [{ start: 18, end: 23 }] }])).toEqual([
    { start: 3, end: 11, marks: [{ start: 5, end: 8 }] },
  ]);
});

it("falls back for an invalid passage and ignores a mark that splits a multibyte character", () => {
  expect(readableCitedPassages("한字", [{ start: 1, end: 6, marks: [] }])).toEqual([]);
  expect(readableCitedPassages("한字", [{ start: 0, end: 6, marks: [{ start: 3, end: 5 }] }])).toEqual([
    { start: 0, end: 2, marks: [] },
  ]);
});

it("focuses the exact UTF-8 range within the passage, including inside a broader mark", () => {
  expect(readableCitedPassages("粗利 73.9%", [
    { start: 0, end: 12, marks: [{ start: 0, end: 12, evidenceId: "ev_other" }] },
  ], { start: 7, end: 12 })).toEqual([
    { start: 0, end: 8, marks: [{ start: 0, end: 3 }, { start: 3, end: 8, focused: true }] },
  ]);
});
