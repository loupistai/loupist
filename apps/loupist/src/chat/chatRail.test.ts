import { railData, railMode } from "./chatRail";
import type { ChatMessage } from "../types";
import type { GrowthEvent, KnowledgeMap } from "../contracts/knowledge";

const map = {
  nodes: ["hanul", "morrow", "bluehaven"].map((id) => ({ id, label: id === "hanul" ? "Hanul Precision" : id, kind: "entity", entityKind: "Organization", degree: 1, watched: false, held: false, localNames: id === "hanul" ? ["한울정밀"] : [] })),
  edges: [{ id: "edge-1", kind: "claim", source: "hanul", target: "morrow", relation: "supplies" }],
  worldRevisionId: "rev",
} as unknown as KnowledgeMap;
const event = (kind: GrowthEvent["kind"], subjectId: string, atMs: number, chat = "chat"): GrowthEvent => ({
  eventId: `${kind}:${subjectId}`, loupistId: "loupist", atMs, kind, subjectId, recordId: kind === "lens_created" ? "lens-1" : "world",
  originChatId: chat, originMessageId: "answer", actor: "loupist", summary: "", beforeSummary: null, afterSummary: null,
});

describe("rail placement", () => {
  it("reserves a context column for wide chats and an inline row for narrow chats", () => {
    expect(railMode(1432, true)).toBe("rail");
    expect(railMode(1120, true)).toBe("rail");
    expect(railMode(1119, true)).toBe("inline");
    expect(railMode(852, true)).toBe("inline");
    expect(railMode(700, true)).toBe("inline");
    expect(railMode(1432, false)).toBe("none");
    expect(railMode(0, true)).toBe("none");
  });
});

describe("rail data", () => {
  const question: ChatMessage = {
    id: "question", role: "user", content: "Compare these", created_at: "2026-10-28T00:00:00Z", status: "complete",
    attachments: [{ id: "a1", filename: "deck.png", media_type: "image/png", size_bytes: 2048 }, { id: "a2", filename: "holdings.csv", media_type: "text/csv", size_bytes: 512 }],
  };
  const answer: ChatMessage = {
    id: "answer", role: "assistant", status: "complete", created_at: "2026-10-28T00:00:10Z",
    content: "I checked [Check cash flow](lens:principle-1) and wrote [the memo](memo.md).",
    lens_receipts: [{ recordId: "principle-1", revisionId: "rev", kind: "principle", title: "Check cash flow", delivery: "context" }, { recordId: "method-1", revisionId: "rev", kind: "method", title: "Separate durable margins", delivery: "exact_read" }],
    memory_refs: [{ read_ref: "r", kind: "lens", title: "Separate durable margins", excerpt: "", access: "exact_read", knowledge: { kind: "lens", recordId: "method-1", revisionId: "rev", objectId: "method-1" } }],
    progress: { startedAtMs: 0, items: [{ id: "s1", kind: "tool", category: "capability", operation: "read", action: "Read filings", status: "succeeded", source: { kind: "connector", name: "Morrow research tools" } }] },
    tool_refusals: [{ connection_id: "c1", connection_name: "Morrow", tool_name: "read_filings", reason: "needs_approval" }],
  } as unknown as ChatMessage;

  it("collects files, what the Loupist used, what it learned here and what waits for the user", () => {
    const data = railData({
      messages: [question, answer], chatId: "chat", language: "ko", workspacePath: "/w",
      map, events: [event("entity_added", "morrow", 1), event("entity_added", "hanul", 2), event("claim_added", "edge-1", 3), event("lens_created", "lens-1", 4), event("entity_added", "bluehaven", 5, "other")],
    });
    expect(data.files.map((file) => [file.filename, file.kind, file.fromLoupist])).toEqual([["deck.png", "image", false], ["holdings.csv", "file", false], ["memo.md", "file", true]]);
    expect(data.using.map((item) => [item.kind, item.label])).toEqual([["principle", "Check cash flow"], ["method", "Separate durable margins"], ["connection", "Morrow research tools"]]);
    // Another chat's growth does not count here; names show their local form.
    expect(data.learned).toMatchObject({ names: 2, methods: 1, chips: [{ id: "hanul", label: "한울정밀" }, { id: "morrow", label: "morrow" }] });
    expect(data.needs).toHaveLength(1);
    expect(data.hasCards).toBe(true);
  });

  it("has no cards for a chat with nothing attached, used or learned", () => {
    const data = railData({ messages: [{ ...question, attachments: [] }], chatId: "chat", language: "en", map, events: [] });
    expect(data).toMatchObject({ files: [], using: [], learned: null, needs: [], hasCards: false });
  });

  it("drops an approval once a newer answer no longer asks for it", () => {
    const later = { ...answer, id: "later", tool_refusals: [] } as ChatMessage;
    expect(railData({ messages: [question, answer, later], chatId: "chat", language: "en" }).needs).toEqual([]);
  });
});
