import i18n from "@/i18n";

const t = i18n.getFixedT(null, "chat");
export const PREVIEW_ROWS = 200;
export const PREVIEW_COLUMNS = 40;

export type DelimitedPreview = {
  rows: string[][];
  rowCount: number;
  columnCount: number;
  incompleteLastRow: boolean;
  error: string | null;
};

/** A bounded, string-only CSV/TSV preview. Quotes follow RFC 4180; values never become numbers or formulas. */
export function delimitedPreview(
  source: string,
  delimiter: "," | "\t" = ",",
  incompleteInput = false,
): DelimitedPreview {
  const text = source.replace(/^\uFEFF/, "");
  const result: DelimitedPreview = { rows: [], rowCount: 0, columnCount: 0, incompleteLastRow: false, error: null };
  let row: string[] = [];
  let field = "";
  let column = 0;
  let quoted = false;
  let afterQuote = false;
  let fieldStarted = false;
  let rowStarted = false;
  const append = (value: string) => {
    if (result.rowCount <= PREVIEW_ROWS && column < PREVIEW_COLUMNS) field += value;
  };
  const finishField = () => {
    if (result.rowCount <= PREVIEW_ROWS && column < PREVIEW_COLUMNS) row.push(field);
    field = ""; column += 1; fieldStarted = false; afterQuote = false;
  };
  const finishRow = () => {
    finishField();
    if (result.rowCount <= PREVIEW_ROWS) result.rows.push(row);
    result.rowCount += 1;
    result.columnCount = Math.max(result.columnCount, column);
    row = []; column = 0; rowStarted = false;
  };
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index]!;
    if (quoted) {
      if (char !== '"') append(char);
      else if (text[index + 1] === '"') { append('"'); index += 1; }
      else { quoted = false; afterQuote = true; }
      continue;
    }
    if (char === delimiter) { finishField(); rowStarted = true; continue; }
    if (char === "\r" || char === "\n") {
      finishRow();
      if (char === "\r" && text[index + 1] === "\n") index += 1;
      continue;
    }
    if (afterQuote || (char === '"' && fieldStarted)) {
      result.error = t("delimitedPreview.aFieldContainsAnUnexpectedQuoteOrTextAfter");
      return result;
    }
    rowStarted = true;
    if (char === '"') quoted = true;
    else append(char);
    fieldStarted = true;
  }
  if (rowStarted && incompleteInput) result.incompleteLastRow = true;
  else if (quoted) result.error = t("delimitedPreview.aQuotedFieldIsNotClosedOpenSourceTo");
  else if (rowStarted) finishRow();
  return result;
}
