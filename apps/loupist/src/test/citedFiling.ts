import type { SourceSpanView } from "../contracts/knowledge";

const policy = "Clipped policy text. 이연 수익은 서비스와 관련됩니다. 递延收入与服务有关。🏷️ ".repeat(21);
const table = "| Category | 2024 | 2022 |\n| --- | --- | --- |\n| Services | 96,169 | 78,129 |\n| Total net sales | 391,035 | 394,328 |";
const cash = "Restricted cash was $700 million.";
const text = [policy, table, policy, cash, policy].join("\n\n");
const utf8 = new TextEncoder();
const bytes = (value: string) => utf8.encode(value).length;
function locate(value: string) {
  const start = bytes(text.slice(0, text.indexOf(value)));
  return { start, end: start + bytes(value) };
}

export const citedFiling: SourceSpanView = {
  spanId: "cited-filing", mediaType: "application/json", selector: "bytes:0-8192", contextNote: "",
  text: JSON.stringify({ content: text }),
  presentation: {
    text, sourcePointer: "/content", extractionMethod: "readability", contentOffset: 12745, nextOffset: null, extractionTruncated: false,
    citedPassages: [
      { ...locate(table), marks: ["96,169", "78,129", "391,035", "394,328"].map(locate) },
      { ...locate(cash), marks: [locate("$700")] },
    ],
  },
};
