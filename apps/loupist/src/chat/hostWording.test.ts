import { act } from "@testing-library/react";
import i18n from "../i18n";
import type { ChatEvidenceRef, ChatMessage, NumberCheck } from "../types";
import { activityTurn, railData } from "./chatRail";
import { evidenceTitle, sourceIdentityLabel } from "./evidence";
import { memoryChangeVerb } from "./memoryActivity";
import { dataRows } from "./numberText";
import { actionLabel } from "./runStatus";

afterEach(async () => { await act(async () => { await i18n.changeLanguage("en"); }); });

const snapshot: ChatEvidenceRef = { id: "ev", title: "NVDA · Quote", kind: "original", tool_name: "mcp__openbb__equity_price_quote", retrieved_at: "2026-11-17T00:00:00Z",
  excerpt: "", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [] };

describe("text the renderer chooses around host steps", () => {
  it("speaks Korean for a host read, a saved record, a quote field, a connection and a live step", async () => {
    await act(async () => { await i18n.changeLanguage("ko"); });
    // A host market-data read is not the web; the Map and Lens are named in the interface language.
    expect(sourceIdentityLabel(snapshot)).toBe("시장 데이터");
    expect(sourceIdentityLabel({ ...snapshot, tool_name: "world_read" })).toBe("지도");
    expect(sourceIdentityLabel({ ...snapshot, tool_name: "web_fetch" })).toBe("웹");
    // The host's record kind ("Source") is never shown as English.
    expect(memoryChangeVerb({ operation: "create", kind: "Source" })).toBe("만든 자료");
    expect(memoryChangeVerb({ operation: "create", kind: "World" })).toBe("만든 지도 항목");
    // A quote's field has a label, and a percent field stays a percent.
    const check = { link_index: 0, text: "156.14", evidence_id: "ev", kind: "data", status: "matched",
      fields: [{ key: "last", value: "156.14", matched: true }, { key: "below_52w_high_pct", value: "-35.8" }] } as NumberCheck;
    expect(dataRows(check, snapshot, [check], "NVDA").map((row) => row.label)).toEqual(["현재가", "52주 최고가 대비"]);
    expect(dataRows(check, snapshot, [check], "NVDA")[1]!.value).toBe("-35.8%");
    const step = { id: "s", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded",
      source: { kind: "connector", name: "Web Research" }, target: "Web Research · Samsung results", startedAtMs: 0, finishedAtMs: 1, href: null } as const;
    const message = { id: "m", role: "assistant", status: "complete", content: "", created_at: "2026-11-17T00:00:00Z",
      progress: { startedAtMs: 0, finishedAtMs: 1, items: [step] } } as unknown as ChatMessage;
    expect(activityTurn(message).groups[0]!.steps[0]!.target).toBe("웹 리서치 · Samsung results");
    expect(railData({ messages: [message], chatId: "c", language: "ko" }).using.map((item) => item.label)).toEqual(["웹 리서치"]);
    // The live line and a step link take the catalog's words, not the host's English.
    expect(actionLabel({ ...step, toolName: "asset_page_read", status: "running", action: "Read the asset page" })).not.toMatch(/[A-Za-z]{3}/u);
  });

  it.each(["en", "ko"] as const)("names a by-direction distance by its side and keeps the older bare key as stored in %s", async language => {
    await act(async () => { await i18n.changeLanguage(language); });
    const words = language === "en"
      ? { older: "Below 52-week high", aboveMa50: "Above 50-day moving average", belowMa200: "Below 200-day moving average" }
      : { older: "52주 최고가 대비", aboveMa50: "50일 이동평균보다 높음", belowMa200: "200일 이동평균보다 낮음" };
    const rows = (fields: NonNullable<NumberCheck["fields"]>) =>
      dataRows({ link_index: 0, text: "10.3%", evidence_id: "ev", kind: "data", status: "matched", fields } as NumberCheck, snapshot, [], "NVDA")
        .map(row => [row.label, row.value]);
    // An older read's bare key held a signed distance: its label and stored sign stay.
    expect(rows([{ key: "below_52w_high_pct", value: "-35.8", matched: true }])).toEqual([[words.older, "-35.8%"]]);
    // A key only the by-direction host sends is named by its side, a zero without a sign.
    expect(rows([{ key: "above_ma50_pct", value: "12.5", matched: true }, { key: "below_ma200_pct", value: "0" }]))
      .toEqual([[words.aboveMa50, "12.5%"], [words.belowMa200, "0%"]]);
    expect(rows([{ key: "below_ma200_pct", value: "-0", matched: true }])).toEqual([[words.belowMa200, "0%"]]);
  });

  it("keeps a market-data read's stored title", () => {
    expect(evidenceTitle(snapshot)).toBe("NVDA · Quote");
  });

  it("never prints a provider's runtime id as a source", () => {
    const headlines: ChatEvidenceRef = { ...snapshot, title: "NVDA · Headlines", provider: "benzinga" };
    expect(sourceIdentityLabel(headlines)).toBe("Benzinga");
    // An id the app has no name for is left out; the read is named by what it is.
    expect(sourceIdentityLabel({ ...headlines, provider: "newsfeed_v2" })).toBe("Market data");
    // A name the host already wrote stays as written.
    expect(sourceIdentityLabel({ ...headlines, provider: "Apple Investor Relations" })).toBe("Apple Investor Relations");
  });

});
