import type { KnowledgeLocation } from "../types";
import { knowledgeLocationFromReference, openKnowledgeLocation, sameKnowledgePin } from "./location";

describe("Chat knowledge reference navigation", () => {
  it("uses the private Journal identity instead of the turn read receipt", () => {
    const location = knowledgeLocationFromReference("loupist-a", {
      kind: "journal", recordId: "", revisionId: "", objectId: "journal-a", receiptId: "read-receipt-b",
    });
    expect(location.selection).toEqual({ kind: "journal", journalId: "journal-a" });
  });

  it("routes a span read without treating a span identity as a Source identity", () => {
    const location = knowledgeLocationFromReference("loupist-a", {
      kind: "source", recordId: "source-module", revisionId: "original-revision", objectId: "span-a", spanId: "span-a",
    });
    expect(location.revisionId).toBe("original-revision");
    expect(location.selection).toEqual({ kind: "source", sourceId: undefined, spanId: "span-a" });
  });

  it("opens a whole World read without selecting a nonexistent node", () => {
    const location = knowledgeLocationFromReference("loupist-a", {
      kind: "world", recordId: "world-module", revisionId: "original-revision", objectId: "world-module",
    });
    expect(location).toEqual({ loupistId: "loupist-a", surface: "world", recordId: "world-module", revisionId: "original-revision" });
  });

  it("navigates between the same passage bound to different World revisions", () => {
    const current: KnowledgeLocation = {
      loupistId: "loupist-a", surface: "source", recordId: "sources", revisionId: "source-revision",
      selection: { kind: "source", sourceId: "source", spanId: "span", worldRecordId: "world", worldRevisionId: "world-old" },
    };
    const next: KnowledgeLocation = {
      ...current,
      selection: { ...current.selection as Extract<KnowledgeLocation["selection"], { kind: "source" }>, worldRevisionId: "world-new" },
    };
    const navigation = openKnowledgeLocation({ current, views: {}, worldSurface: "world" as const, past: [], generation: 0 }, next);
    expect(navigation.current).toBe(next);
    expect(navigation.past).toEqual([current]);
  });

  it("keeps snapshot revisions and delimited identities distinct", () => {
    const pin = { kind: "source" as const, loupistId: "loupist", recordId: "module", revisionId: "old", objectId: "source", label: "Source" };
    const current: KnowledgeLocation = { loupistId: "loupist", surface: "note", selection: { kind: "snapshot", pin } };
    const next: KnowledgeLocation = { ...current, selection: { kind: "snapshot", pin: { ...pin, revisionId: "new" } } };
    expect(openKnowledgeLocation({ current, views: {}, worldSurface: "world" as const, past: [], generation: 0 }, next).current).toBe(next);
    expect(sameKnowledgePin({ ...pin, recordId: "a:b", revisionId: "c" }, { ...pin, recordId: "a", revisionId: "b:c" })).toBe(false);
  });
});
