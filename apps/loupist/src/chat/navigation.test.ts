import { describe, expect, it } from "vitest";
import type { ChatEvidenceRef, MemoryRef } from "../types";
import { evidenceFixture } from "../test/evidenceFixtures";
import { destinationIcon, evidenceTarget, externalUrl, hostSourceDestination, wantsExternal, type ChatDestination } from "./navigation";
import { tabShortcut } from "../app/shortcuts";

const web = evidenceFixture({ id: "ev_web", tool_name: "web_fetch", url: "https://example.com/report" });
const filing = evidenceFixture({ id: "ev_filing", tool_name: "dart_filing", url: "https://dart.fss.or.kr/x" });
const quote = evidenceFixture({ id: "ev_quote", tool_name: "mcp__openbb__equity_price_quote", url: undefined,
  excerpt: JSON.stringify({ results: [{ symbol: "AAPL", currency: "USD", last_price: 201.5 }] }) });
const derived = evidenceFixture({ id: "ev_calc", kind: "derived", tool_name: "calculate", url: undefined });
const lensRead = evidenceFixture({ id: "ev_lens", kind: "saved_context", tool_name: "lens_read", url: undefined, read_ref: "read-lens" });
const lensSearch = evidenceFixture({ id: "ev_search", kind: "saved_context", tool_name: "lens_search", url: undefined });
const assetRead = evidenceFixture({ id: "ev_asset", kind: "saved_context", tool_name: "asset_page_read", url: undefined, title: "Asset page: Apple",
  excerpt: "{\n  \"assetId\": \"NASDAQ:AAPL\",\n  \"name\": \"Apple\"\n}" });
const assetReadTruncated = evidenceFixture({ id: "ev_asset2", kind: "saved_context", tool_name: "asset_page_read", url: undefined, excerpt: "{\n  \"exists\": " });
const datasetRead = evidenceFixture({ id: "ev_dataset", kind: "saved_context", tool_name: "journal_read", url: undefined });
const lensMemory: MemoryRef = { read_ref: "read-lens", kind: "lens", title: "Moats", excerpt: "", access: "exact_read",
  knowledge: { kind: "lens", recordId: "lens-1", revisionId: "rev-1", objectId: "lens-1" }, as_of: null, section: null };

describe("evidenceTarget", () => {
  const cases: Array<[string, ChatEvidenceRef, MemoryRef[], ReturnType<typeof evidenceTarget>["kind"]]> = [
    ["a fetched web page opens the page", web, [], "browser"],
    ["a filing with a link opens the retained reader", filing, [], "reader"],
    ["a quote basket opens its popover", quote, [], "quote"],
    ["a calculation reveals its step", derived, [], "step"],
    ["an exact Memory read opens Memory", lensRead, [lensMemory], "memory"],
    ["a Memory receipt with no read reveals its step", lensRead, [], "step"],
    ["a Memory search reveals its step", lensSearch, [], "step"],
    ["an asset page read opens the asset page", assetRead, [], "asset"],
    ["an unidentifiable asset page read opens the reader", assetReadTruncated, [], "reader"],
    ["other saved context opens the reader", datasetRead, [], "reader"],
  ];
  it.each(cases)("%s", (_name, evidence, refs, kind) => {
    expect(evidenceTarget(evidence, refs, "loupist-1").kind).toBe(kind);
  });

  it("names the asset and the Memory location it opens", () => {
    expect(evidenceTarget(assetRead, [], "loupist-1")).toEqual({ kind: "asset", target: { loupistId: "loupist-1", assetId: "NASDAQ:AAPL" } });
    expect(evidenceTarget(lensRead, [lensMemory], "loupist-1")).toMatchObject({ kind: "memory", title: "Moats", location: { loupistId: "loupist-1", surface: "lens", recordId: "lens-1" } });
  });
});

describe("hostSourceDestination", () => {
  const asset = { loupistId: "l", assetId: "NASDAQ:AAPL" };
  it("opens a source with a page through the evidence path, falling back to the asset page", () => {
    expect(hostSourceDestination(filing, asset)).toEqual({ kind: "evidence", evidence: filing, fallback: { kind: "asset", target: asset } });
  });
  it("opens the asset page for a source with no page", () => {
    expect(hostSourceDestination(derived, asset)).toEqual({ kind: "asset", target: asset });
  });
});

describe("externalUrl", () => {
  const cases: Array<[string, ChatDestination, string | null]> = [
    ["a web link", { kind: "link", url: "https://example.com/a#b" }, "https://example.com/a#b"],
    ["a link with credentials", { kind: "link", url: "https://user:pw@example.com/" }, null],
    ["a non-http link", { kind: "link", url: "javascript:alert(1)" }, null],
    ["evidence with a source address", { kind: "evidence", evidence: filing }, "https://dart.fss.or.kr/x"],
    ["evidence without one", { kind: "evidence", evidence: derived }, null],
    ["evidence falling back to a link", { kind: "evidence", evidence: derived, fallback: { kind: "link", url: "https://example.com/" } }, "https://example.com/"],
    ["an artifact", { kind: "artifact", artifact: { artifact_id: "a", revision: 1, kind: "markdown", title: "Memo", digest: "d" } }, null],
    ["an asset page", { kind: "asset", target: { loupistId: "l", assetId: "a" } }, null],
  ];
  it.each(cases)("%s", (_name, to, url) => expect(externalUrl(to)).toBe(url));
});

describe("destinationIcon", () => {
  const cases: Array<[string, ChatDestination, boolean, ReturnType<typeof destinationIcon>]> = [
    ["a link opens the panel browser", { kind: "link", url: "https://example.com/" }, false, "panel"],
    ["a link asked for the system browser", { kind: "link", url: "https://example.com/" }, true, "external"],
    ["a web citation", { kind: "evidence", evidence: web }, false, "panel"],
    ["a filing citation", { kind: "evidence", evidence: filing }, false, "panel"],
    ["a filing citation, external", { kind: "evidence", evidence: filing }, true, "external"],
    ["a calculation stays in the message", { kind: "evidence", evidence: derived }, false, null],
    ["a calculation has no browser to open", { kind: "evidence", evidence: derived }, true, null],
    ["a quote opens its popover", { kind: "evidence", evidence: quote }, false, null],
    ["an asset page read replaces the view", { kind: "evidence", evidence: assetRead }, false, null],
    ["Memory opens the panel", { kind: "memory", location: { loupistId: "l", surface: "lens", recordId: "r" }, title: "Lens" }, false, "panel"],
    ["a Lens record opens the panel", { kind: "lens", recordId: "r" }, false, "panel"],
    ["a file opens the panel", { kind: "file", path: "notes.md" }, false, "panel"],
    ["a full page replaces the view", { kind: "page", location: { loupistId: "l", surface: "world" } }, false, null],
    ["a schedule replaces the view", { kind: "schedule", taskId: "t" }, false, null],
    ["another chat replaces the view", { kind: "chat", threadId: "t" }, false, null],
  ];
  it.each(cases)("%s", (_name, to, external, icon) => expect(destinationIcon(to, { external })).toBe(icon));
});

describe("wantsExternal", () => {
  it.each(["MacIntel", "iPad", "Linux x86_64"])("uses the same platform modifier as shortcuts on %s", platform => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue(platform);
    const mac = platform !== "Linux x86_64";
    expect(tabShortcut(1)).toBe(mac ? "⌘1" : "Ctrl+1");
    expect(wantsExternal({ metaKey: true, ctrlKey: false })).toBe(mac);
    expect(wantsExternal({ metaKey: false, ctrlKey: true })).toBe(!mac);
  });
  const click = (overrides: Partial<{ button: number; metaKey: boolean; ctrlKey: boolean }>) => ({ button: 0, metaKey: false, ctrlKey: false, ...overrides });
  const cases: Array<[string, ReturnType<typeof click>, boolean, boolean]> = [
    ["a plain click on macOS", click({}), true, false],
    ["Cmd-click on macOS", click({ metaKey: true }), true, true],
    ["Ctrl-click on macOS is a context click", click({ ctrlKey: true }), true, false],
    ["Ctrl-click elsewhere", click({ ctrlKey: true }), false, true],
    ["the Windows key elsewhere", click({ metaKey: true }), false, false],
    ["a middle click anywhere", click({ button: 1 }), false, true],
  ];
  it.each(cases)("%s", (_name, event, mac, expected) => expect(wantsExternal(event, mac)).toBe(expected));
});

it("opens host artifact reads even with truncated bodies, without taking ids from document prose", () => {
  const id = `artifact-${"a".repeat(32)}`;
  const receipt = (excerpt: string) => evidenceFixture({ kind: "derived", tool_name: "artifact_read", url: undefined, excerpt });
  for (const metadata of [{ id, kind: "markdown", title: 'Quoted "title" }' }, { artifact_id: id, kind: "chart" }, { artifact_id: id, kind: "grid" }]) {
    const evidence = receipt(`{"artifact":${JSON.stringify(metadata)},"revision":{"payload":"truncated`);
    expect(evidenceTarget(evidence, [], "loupist")).toEqual({ kind: "artifact", artifactId: id });
    expect(destinationIcon({ kind: "evidence", evidence })).toBe("panel");
  }
  for (const excerpt of [JSON.stringify({ body: { artifact: { id, kind: "markdown" } } }), '{"artifact":{"id":"unfinished', JSON.stringify({ artifact: { id: "../outside", kind: "markdown" } }), JSON.stringify({ artifact: { id, kind: "unknown" } })]) {
    expect(evidenceTarget(receipt(excerpt), [], "loupist").kind).toBe("step");
  }
});
