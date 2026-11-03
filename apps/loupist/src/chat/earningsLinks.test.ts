import type { ChatEvidenceRef, NumberCheck } from "../contracts/chat";
import { earningsCards } from "./earningsLinks";

const ref = (id: string, tool: string, readRef?: string): ChatEvidenceRef => ({
  id, title: id, kind: "saved_context", tool_name: tool, read_ref: readRef, retrieved_at: "2026-10-30T00:00:00Z",
  excerpt: "", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [],
});
const SNAPSHOT = `ev_${"e".repeat(32)}`;
const OTHER = `ev_${"a".repeat(32)}`;
const check = (link_index: number, text: string): NumberCheck => ({ link_index, text, evidence_id: OTHER, kind: "data", status: "matched" });

it("turns a paragraph that only links to a results snapshot into a card and keeps later number checks on their links", () => {
  const content = `Revenue rose to [1.27bn](#evidence-${OTHER}).\n\n[Results · Q3 2026](#evidence-${SNAPSHOT})\n\nMargin was [14.8%](#evidence-${OTHER}).`;
  const result = earningsCards(content, [ref(OTHER, "web_fetch"), ref(SNAPSHOT, "earnings_snapshot", "earnings:ast_0123456789abcdef:evt-q3")], [check(0, "1.27bn"), check(2, "14.8%")]);
  expect(result.cards).toEqual([{ evidenceId: SNAPSHOT, assetId: "ast_0123456789abcdef", eventId: "evt-q3" }]);
  expect(result.text).toBe(`Revenue rose to [1.27bn](#evidence-${OTHER}).\n\nMargin was [14.8%](#evidence-${OTHER}).`);
  expect(result.numberChecks?.map(item => [item.link_index, item.text])).toEqual([[0, "1.27bn"], [1, "14.8%"]]);
});

it("leaves a snapshot cited inside a sentence, and other evidence, as they are", () => {
  const content = `See [the results](#evidence-${SNAPSHOT}) for details.`;
  const result = earningsCards(content, [ref(SNAPSHOT, "earnings_snapshot", "earnings:ast_0123456789abcdef:evt-q3")], []);
  expect(result.cards).toEqual([]);
  expect(result.text).toBe(content);
  expect(earningsCards(`[x](#evidence-${OTHER})`, [ref(OTHER, "web_fetch")]).cards).toEqual([]);
});
