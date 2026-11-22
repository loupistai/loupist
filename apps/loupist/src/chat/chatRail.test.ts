import { activityTurn, railData } from "./chatRail";
import type { ChatMessage } from "../types";
import type { GrowthEvent, KnowledgeMap } from "../contracts/knowledge";

const map = {
  nodes: ["hanul", "morrow", "bluehaven"].map((id) => ({ id, label: id === "hanul" ? "Hanul Precision" : id, kind: "entity", entityKind: "Organization", degree: 1, watched: false, localNames: id === "hanul" ? ["한울정밀"] : [] })),
  edges: [{ id: "edge-1", kind: "claim", source: "hanul", target: "morrow", relation: "supplies" }],
  worldRevisionId: "rev",
} as unknown as KnowledgeMap;
const event = (kind: GrowthEvent["kind"], subjectId: string, atMs: number, chat = "chat"): GrowthEvent => ({
  eventId: `${kind}:${subjectId}`, loupistId: "loupist", atMs, kind, subjectId, recordId: kind === "lens_created" ? "lens-1" : "world",
  originChatId: chat, originMessageId: "answer", actor: "loupist", summary: "", beforeSummary: null, afterSummary: null,
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

  it.each([true, false])("keeps saved activity and gates pending approvals (custom connections available: %s)", customConnections => {
    const data = railData({
      messages: [question, answer], customConnections, chatId: "chat", language: "ko", workspacePath: "/w",
      map, events: [event("entity_added", "morrow", 1), event("entity_added", "hanul", 2), event("claim_added", "edge-1", 3), event("lens_created", "lens-1", 4), event("entity_added", "bluehaven", 5, "other")],
    });
    expect(data.files.map((file) => [file.filename, file.kind, file.fromLoupist])).toEqual([["deck.png", "image", false], ["holdings.csv", "file", false], ["memo.md", "file", true]]);
    expect(data.using.map((item) => [item.kind, item.label])).toEqual([["principle", "Check cash flow"], ["method", "Separate durable margins"], ["connection", "Morrow research tools"]]);
    // Another chat's growth does not count here; names show their local form.
    expect(data.learned).toMatchObject({ names: 2, methods: 1, chips: [{ id: "hanul", label: "한울정밀" }, { id: "morrow", label: "morrow" }] });
    expect(data.needs).toHaveLength(customConnections ? 1 : 0);
    expect(data.hasCards).toBe(true);
  });

  it("counts only the waiting questions of the latest answer as needing the user", () => {
    const ask = (state: "waiting" | "answered") => ({ requestId: `q-${state}`, question: "Which period?", options: [{ label: "A" }, { label: "B" }], state, requestedAtMs: 1 });
    const asking = { ...answer, tool_refusals: [], progress: { ...answer.progress, questions: [ask("waiting"), ask("answered")] } } as ChatMessage;
    const data = railData({ messages: [question, asking], chatId: "chat", language: "en", map, events: [] });
    expect(data.questions.map((item) => item.requestId)).toEqual(["q-waiting"]);
    expect(data.hasCards).toBe(true);
    const settled = railData({ messages: [question, { ...asking, progress: { ...asking.progress, questions: [ask("answered")] } } as ChatMessage], chatId: "chat", language: "en", map, events: [] });
    expect(settled.questions).toEqual([]);
  });

  it("has no cards for a chat with nothing attached, used or learned", () => {
    const data = railData({ messages: [{ ...question, attachments: [] }], chatId: "chat", language: "en", map, events: [] });
    expect(data).toMatchObject({ files: [], using: [], learned: null, needs: [], hasCards: false });
  });

  it("drops an approval once a newer answer no longer asks for it", () => {
    const later = { ...answer, id: "later", tool_refusals: [] } as ChatMessage;
    expect(railData({ messages: [question, answer, later], customConnections: true, chatId: "chat", language: "en" }).needs).toEqual([]);
  });
});

describe("activity turn", () => {
  const step = (id: string, toolName: string, extra: Record<string, unknown> = {}) => ({
    id, kind: "tool", toolName, category: "finance", operation: "read", action: `Read ${toolName.replaceAll("_", " ")}`, target: null, href: null,
    status: "succeeded", startedAtMs: 1_000, finishedAtMs: 1_400, ...extra,
  });
  const reply = (items: unknown[], extra: Partial<ChatMessage> = {}) => ({
    id: "reply", role: "assistant", status: "complete", content: "", created_at: "2026-10-28T00:00:10Z",
    progress: { startedAtMs: 0, finishedAtMs: 9_000, items }, ...extra,
  }) as unknown as ChatMessage;

  it("collapses steps of the same kind into one row with a count, in the order they first ran", () => {
    const turn = activityTurn(reply([
      step("q1", "mcp__openbb__equity_price_quote", { target: "NVDA" }),
      step("c1", "calculate", { category: "compute", operation: "calculate" }),
      step("q2", "mcp__openbb__equity_price_quote", { target: "AMD", finishedAtMs: 1_900 }),
      step("c2", "calculate", { category: "compute", operation: "calculate", finishedAtMs: null }),
      step("plan", "plan_update", { category: "system", operation: "generic" }),
      { id: "note", kind: "commentary", text: "Checking the quote" },
    ]));
    expect(turn.stepCount).toBe(4);
    expect(turn.groups.map((group) => [group.label, group.steps.length, group.durationMs])).toEqual([
      ["Read current quote", 2, 1_300],
      // One calculation has no end time, so the row has no total.
      ["Calculated", 2, undefined],
    ]);
    expect(turn.groups[0]!.steps.map((item) => item.target)).toEqual(["NVDA", "AMD"]);
    expect(turn.durationMs).toBe(9_000);
  });

  it("never shows a tool's raw name: unknown reads, connections and identifier targets get localized words", () => {
    const turn = activityTurn(reply([
      step("a", "mcp__openbb__activate_tools", { category: "capability", operation: "generic", target: "news_company, equity_estimates_forward_eps" }),
      step("n", "mcp__openbb__news_company", { target: "NVDA" }),
      step("w", "mcp__web_research__lookup", { category: "capability", operation: "search", source: { kind: "connector", name: "Web Research" } }),
      step("x", "mcp__other__mystery_tool", { category: "capability", operation: "generic" }),
    ]));
    expect(turn.groups.map((group) => group.label)).toEqual(["Prepared data tools", "Read company news", "Used Web Research", "Did a step"]);
    expect(turn.groups[0]!.steps[0]!.target).toBe("company news, earnings estimates");
    expect(JSON.stringify(turn.groups)).not.toMatch(/activate|news_company|mystery/u);
  });

  it("keeps zero and sub-second times and reads a row's safe failure reason", () => {
    const turn = activityTurn(reply([
      step("zero", "calculate", { finishedAtMs: 1000 }),
      step("fast", "calculate", { finishedAtMs: 1040 }),
      step("offline", "web_fetch", { status: "failed", failureReason: "unreachable" }),
    ]));
    expect(turn.groups[0]!.steps.map(step => step.durationMs)).toEqual([0, 40]);
    expect(turn.groups[1]!.steps[0]!.reason).toBe("The data provider could not be reached. It was not used in the answer.");
  });

  it("gives web and market-data SEC refusals the contact setup reason", () => {
    const tools = ["web_fetch", "mcp__openbb__equity_fundamental_income"];
    const turn = activityTurn(reply(tools.map((tool) => step(tool, tool, {
      status: "failed",
      failureReason: "invalid_input",
      refusal: "sec_contact_required",
    }))));
    expect(turn.groups.map((group) => group.steps[0]!.reason)).toEqual([
      "SEC contact email. Set up in Data connections.",
      "SEC contact email. Set up in Data connections.",
    ]);
  });

  it("gives every failed step a row of its own with a short reason", () => {
    const turn = activityTurn(reply([
      step("ok", "mcp__openbb__equity_price_quote"),
      step("bad", "mcp__openbb__equity_price_quote", { status: "failed", finishedAtMs: 3_000 }),
      step("file", "read", { category: "files", status: "failed", refusal: "protected_path" }),
      step("conn", "mcp__morrow__read_filings", { category: "capability", status: "failed", source: { kind: "connector", name: "Morrow" } }),
    ], { tool_refusals: [{ connection_id: "c", connection_name: "Morrow", tool_name: "read_filings", reason: "needs_approval" }] }));
    expect(turn.groups.map((group) => [group.label, group.steps.length, group.status, group.steps[0]!.reason])).toEqual([
      ["Read current quote", 1, "succeeded", undefined],
      ["Read current quote", 1, "failed", "It ended with an error. It was not used in the answer."],
      ["Read a file", 1, "failed", "The path is in the app's private data, so it was not opened."],
      ["Used Morrow", 1, "failed", "Using this connection needs your OK."],
    ]);
    expect(turn.groups[1]!.durationMs).toBe(2_000);
  });

  it("says a failed revision of a shown table, chart or document left the earlier version, and keeps other reasons", () => {
    const publish = (id: string, toolName: string, target: string, extra: Record<string, unknown> = {}) =>
      step(id, toolName, { category: "artifact", operation: "publish", target, status: "failed", ...extra });
    const kept = "The change was not applied. The earlier version is shown.";
    const generic = "It ended with an error. It was not used in the answer.";
    const reasons = (message: ChatMessage) => activityTurn(message).groups.map((group) => group.steps[0]!.reason);
    const ref = (title: string, kind = "grid") => ({ artifact_id: title, revision: 1, kind, title, digest: "a".repeat(64) });
    const view = (title: string) => ({ artifact: { id: title, title, kind: "grid" }, revision: { revision: 1, digest: "a".repeat(64) } });
    // Stored: each publish tool, matched by its trimmed title.
    expect(reasons(reply([
      publish("g", "grid_publish", " Payers "), publish("c", "chart_publish", "Margin"), publish("d", "artifact_publish", "Note"),
    ], { artifact_refs: [ref("Payers"), ref("Margin", "chart"), ref("Note", "markdown")] } as Partial<ChatMessage>))).toEqual([kept, kept, kept]);
    // Streaming: the staged draft is what the reply shows.
    expect(reasons(reply([publish("g", "grid_publish", "Payers")], { status: "streaming", staged_artifacts: [view("Payers")] } as unknown as Partial<ChatMessage>))).toEqual([kept]);
    // A stopped reply shows no staged draft, so its failure keeps today's reason.
    expect(reasons(reply([publish("g", "grid_publish", "Payers")], { status: "aborted", staged_artifacts: [view("Payers")] } as unknown as Partial<ChatMessage>))).toEqual([generic]);
    // Nothing of that title is shown, or the step is not a publish: today's reason.
    expect(reasons(reply([publish("g", "grid_publish", "Other"), step("r", "web_fetch", { status: "failed", target: "Payers" })],
      { artifact_refs: [ref("Payers")] } as Partial<ChatMessage>))).toEqual([generic, generic]);
    // Stopped and safety refusals keep their precedence.
    expect(reasons(reply([
      publish("s", "grid_publish", "Payers", { status: "stopped" }),
      publish("p", "grid_publish", "Payers", { refusal: "untrusted_text_paused" }),
    ], { artifact_refs: [ref("Payers")] } as Partial<ChatMessage>))).toEqual(["Stopped before it finished.", "Paused by the safety rule after the answer read another site's text."]);
  });

  it.each([["grid_publish", "grid"], ["chart_publish", "chart"], ["artifact_publish", "markdown"]] as const)(
    "says the earlier version is shown only when no later %s of that title succeeded", (toolName, kind) => {
      const publish = (id: string, status: "failed" | "succeeded" | "stopped", extra: Record<string, unknown> = {}) =>
        step(id, toolName, { category: "artifact", operation: "publish", target: "Payers", status, ...extra });
      const kept = "The change was not applied. The earlier version is shown.";
      const generic = "It ended with an error. It was not used in the answer.";
      const shown = { artifact_refs: [{ artifact_id: "a", revision: 1, kind, title: "Payers", digest: "a".repeat(64) }] } as Partial<ChatMessage>;
      const reasons = (...items: unknown[]) => Object.fromEntries(activityTurn(reply(items, shown)).groups.flatMap((group) => group.steps).map((item) => [item.id, item.reason]));
      // Failed, then a publish of the same title succeeded: the card shows that success, so the failure reads as before.
      expect(reasons(publish("first", "failed"), publish("then", "succeeded"))).toEqual({ first: generic, then: undefined });
      // Succeeded, then a revision failed: the earlier version is what the card shows.
      expect(reasons(publish("first", "succeeded"), publish("revise", "failed"))).toEqual({ first: undefined, revise: kept });
      // Two failed revisions after a success: no later success replaced the card, so both say the earlier version stays.
      expect(reasons(publish("first", "succeeded"), publish("a", "failed"), publish("b", "failed"))).toEqual({ first: undefined, a: kept, b: kept });
      // A later success of another title does not count.
      expect(reasons(publish("first", "succeeded"), publish("revise", "failed"), publish("other", "succeeded", { target: "Other" }))).toMatchObject({ revise: kept });
      // Stopped and safety refusals still come first.
      expect(reasons(publish("first", "succeeded"), publish("s", "stopped"), publish("p", "failed", { refusal: "untrusted_text_paused" }))).toEqual({
        first: undefined, s: "Stopped before it finished.", p: "Paused by the safety rule after the answer read another site's text.",
      });
    });

  it("lists cited sources once and pairs each checked figure with its evidence", () => {
    const evidence = { id: "e1", title: "Quote", kind: "original", tool_name: "web_fetch", url: "https://example.com/q", retrieved_at: "2026-10-28T00:00:00Z", excerpt: "", excerpt_truncated: false, content_sha256: "", warnings: [] };
    const turn = activityTurn(reply([step("q", "web_fetch", { category: "web", resultIds: ["e1"] })], {
      evidence_refs: [evidence, { ...evidence, id: "e2" }] as ChatMessage["evidence_refs"],
      number_checks: [{ link_index: 0, text: "$162.85", evidence_id: "e1", kind: "data", status: "matched" }] as ChatMessage["number_checks"],
    }));
    expect(turn.sources.map((source) => source.evidence.id)).toEqual(["e1"]);
    expect(turn.groups[0]!.steps[0]!.evidence.map((item) => item.id)).toEqual(["e1"]);
    expect(turn.checks).toEqual([{ check: expect.objectContaining({ text: "$162.85" }), evidence: expect.objectContaining({ id: "e1" }) }]);
  });
});
