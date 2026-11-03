import type { ChatEvidenceRef } from "../types";
export const evidenceFixture = (overrides: Partial<ChatEvidenceRef> = {}): ChatEvidenceRef => ({
  id: `ev_${"a".repeat(32)}`, title: "Apple FY2025 annual report", kind: "original",
  url: "https://investor.apple.com/report.pdf", provider: "Apple Investor Relations",
  tool_name: "web_fetch", retrieved_at: "2026-10-11T03:00:00Z", published_at: "2025-10-31",
  as_of: "FY2025 · USD millions", excerpt: "Total net sales 416,161. Operating cash flow 111,482.",
  excerpt_truncated: false, content_sha256: "b".repeat(64), warnings: [], ...overrides,
});
