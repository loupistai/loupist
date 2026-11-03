/** A separate page title already displays an identical opening line. */
export const bodyWithoutRepeatedTitle = (title: string, markdown: string) => {
  const body = markdown.trimStart();
  const lineEnd = body.indexOf("\n");
  const firstLine = (lineEnd < 0 ? body : body.slice(0, lineEnd)).replace(/^#{1,6}\s+/, "").replace(/\s+#+\s*$/, "").trim();
  return firstLine === title.trim() ? (lineEnd < 0 ? "" : body.slice(lineEnd + 1).trimStart()) : markdown;
};
