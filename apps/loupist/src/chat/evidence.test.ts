import { evidenceFixture } from "../test/evidenceFixtures";
import { citationLabel, citationPageUrl, citationSources, evidenceExcerptParts, evidenceTitle, webEvidenceUrl } from "./evidence";
import { isInlineWorkEvidence, memoryReadForEvidence } from "./memoryActivity";

it("opens a host website address and ignores titles, finance, files and search previews", () => {
  expect(webEvidenceUrl(evidenceFixture({ url: "https://example.com/a" }))).toBe("https://example.com/a");
  expect(citationPageUrl(evidenceFixture({ tool_name: "server__web_fetch", url: "https://issuer.example/q3" }))).toBe("https://issuer.example/q3");
  expect(citationPageUrl(evidenceFixture({ tool_name: "web_read", url: "https://issuer.example/notes" }))).toBe("https://issuer.example/notes");
  expect(citationPageUrl(evidenceFixture({ title: "https://evil.example/phish", url: undefined }))).toBeNull();
  expect(citationPageUrl(evidenceFixture({ tool_name: "finance_market_data", url: "https://finance.example/filing" }))).toBeNull();
  expect(citationPageUrl(evidenceFixture({
    tool_name: "equity_price_quote", url: "https://finance.example/quotes",
    excerpt: JSON.stringify({ results: [{ symbol: "AAPL", last_price: 1 }] }),
  }))).toBeNull();
  expect(citationPageUrl(evidenceFixture({ kind: "local_file", url: "https://example.com/file" }))).toBeNull();
  expect(citationPageUrl(evidenceFixture({ kind: "discovery", tool_name: "web_search", url: "https://example.com/hit" }))).toBeNull();
  expect(citationPageUrl(evidenceFixture({ url: "javascript:alert(1)" }))).toBeNull();
  expect(citationPageUrl(evidenceFixture({ url: "https://user:secret@example.com/a" }))).toBeNull();
});

it("uses the actual filename for URL-only titles without changing human titles", () => {
  expect(evidenceTitle(evidenceFixture({ title: "https://issuer.example/long/path/Annual%20Report.pdf" }))).toBe("Annual Report.pdf");
  expect(evidenceTitle(evidenceFixture({ title: "Issuer annual report" }))).toBe("Issuer annual report");
});

it("maps host byte offsets into exact Unicode text without changing the excerpt", () => {
  const before = "한국어 🌎\n";
  const match = "영업현금흐름 111,482";
  const excerpt = before + match + "\nUSD millions";
  const start = new TextEncoder().encode(before).length;
  const end = start + new TextEncoder().encode(match).length;
  const parts = evidenceExcerptParts(evidenceFixture({ excerpt, excerpt_match: { start, end } }));
  expect(parts).toEqual({ before, match, after: "\nUSD millions" });
  expect(parts!.before + parts!.match + parts!.after).toBe(excerpt);
});

it("numbers retrieved sources and leaves search, Memory and execution unnumbered", () => {
  const calculated = evidenceFixture({ id: `ev_${"b".repeat(32)}`, kind: "derived", tool_name: "chart_query", title: "Margin calculation" });
  const web = evidenceFixture({ id: `ev_${"a".repeat(32)}`, title: "Rates outlook", tool_name: "web_fetch" });
  const read = evidenceFixture({ id: `ev_${"d".repeat(32)}`, kind: "saved_context", tool_name: "lens_read", title: "lens read", url: undefined });
  const preview = evidenceFixture({ id: `ev_${"c".repeat(32)}`, kind: "discovery", tool_name: "web_search", title: "Search preview", url: undefined });
  const search = evidenceFixture({ id: `ev_${"e".repeat(32)}`, kind: "saved_context", tool_name: "world_search", title: "world search", url: undefined });
  const file = evidenceFixture({ id: `ev_${"f".repeat(32)}`, kind: "local_file", tool_name: "workspace_capture", title: "notes.md", url: undefined });
  const all = [calculated, web, read, preview, search, file];
  expect(citationSources(all).map((item) => item.title)).toEqual(["Rates outlook", "notes.md"]);
  expect(citationLabel(web, all)).toBe("Source 1: Rates outlook");
  expect(citationLabel(file, all)).toBe("Source 2: notes.md");
  expect(isInlineWorkEvidence(preview, [])).toBe(true);
  expect(citationLabel(preview, all)).toBe("Result: Search preview");
  expect(citationLabel(read, all, "Margin method")).toBe("Memory: Margin method");
  expect(citationLabel(search, all)).toBe("Result: Map search");
  expect(citationLabel(calculated, all)).toBe("Result: Margin calculation");
});

it("pairs a Memory receipt with the exact read that shares its readRef", () => {
  const search = evidenceFixture({ id: `ev_${"1".repeat(32)}`, kind: "saved_context", tool_name: "conversation_memory", title: "conversation memory", url: undefined });
  const journal = evidenceFixture({ id: `ev_${"2".repeat(32)}`, kind: "saved_context", tool_name: "conversation_memory", title: "conversation memory", url: undefined, read_ref: "journal-1" });
  const first = evidenceFixture({ id: `ev_${"3".repeat(32)}`, kind: "saved_context", tool_name: "world_read", title: "world read", url: undefined, read_ref: "filing" });
  const again = evidenceFixture({ id: `ev_${"4".repeat(32)}`, kind: "saved_context", tool_name: "world_read", title: "world read", url: undefined, read_ref: "filing-again" });
  const unmatched = evidenceFixture({ id: `ev_${"5".repeat(32)}`, kind: "saved_context", tool_name: "world_read", title: "world read", url: undefined });
  const reads = [
    { read_ref: "journal-1", kind: "journal" as const, title: "Capacity review", excerpt: "", as_of: null, section: null, access: "exact_read" as const, knowledge: { kind: "journal" as const, recordId: "d1", revisionId: "r", objectId: "d1" } },
    { read_ref: "filing", kind: "source" as const, title: "Filing", excerpt: "", as_of: null, section: null, access: "exact_read" as const, knowledge: { kind: "source" as const, recordId: "s1", revisionId: "r", objectId: "o1" } },
    { read_ref: "filing-again", kind: "source" as const, title: "Filing", excerpt: "", as_of: null, section: null, access: "exact_read" as const, knowledge: { kind: "source" as const, recordId: "s1", revisionId: "r", objectId: "o1" } },
    { read_ref: "map", kind: "world" as const, title: "Map", excerpt: "", as_of: null, section: null, access: "exact_read" as const, knowledge: { kind: "world" as const, recordId: "w1", revisionId: "r", objectId: "w1" } },
  ];
  expect(memoryReadForEvidence(search, reads)).toBeNull();
  expect(memoryReadForEvidence(journal, reads)?.title).toBe("Capacity review");
  expect(memoryReadForEvidence(first, reads)?.title).toBe("Filing");
  expect(memoryReadForEvidence(again, reads)?.title).toBe("Filing");
  expect(memoryReadForEvidence(unmatched, reads)).toBeNull();
  expect(isInlineWorkEvidence(search, reads)).toBe(true);
  expect(isInlineWorkEvidence(unmatched, reads)).toBe(true);
  expect(isInlineWorkEvidence(journal, reads)).toBe(false);
});

it("does not highlight invalid, out-of-range or split-character offsets", () => {
  for (const excerpt_match of [{ start: -1, end: 3 }, { start: 0, end: 100 }, { start: 0, end: 1 }, { start: 2, end: 2 }, { start: 0.5, end: 3 }]) {
    expect(evidenceExcerptParts(evidenceFixture({ excerpt: "한글", excerpt_match }))).toBeNull();
  }
});
