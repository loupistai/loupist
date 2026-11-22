import { chatMarkdown, datasetCellText, datasetCsv, exportFileName } from "./exportChat";
import type { ChatThread } from "../types";

it("writes a Markdown transcript with roles, times, citations as footnotes and the AI notice last", () => {
  const thread: ChatThread = {
    id: "chat", loupist_id: "loupist", title: "Hanul margin review", updated_at: "2026-10-28T00:00:00Z", use_memory: true, update_memory: true,
    messages: [
      { id: "q", role: "user", content: "Why did the margin fall?", created_at: "2026-10-28T00:00:00Z", status: "complete" },
      { id: "a", role: "assistant", content: "Qualification costs rose. [1](#evidence-e1)", created_at: "2026-10-28T00:00:12Z", status: "complete",
        execution_model: { profile_id: "openai/gpt-6.1-sol", name: "GPT-6.1 Sol", provider: "openai", model: "gpt-6.1-sol", thinking_level: "medium", run_options: {} },
        evidence_refs: [{ id: "e1", title: "Q3 operating review", kind: "original", url: "https://hanul.example/q3", provider: "IR", tool_name: "web_fetch", retrieved_at: "2026-10-28T00:00:00Z", published_at: "2026-10-27", excerpt: "", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [] }] },
    ],
  };
  const markdown = chatMarkdown(thread, "Quality Compounder", Date.parse("2026-10-28T00:00:00Z"));
  expect(markdown).toMatch(/^# Hanul margin review\n\nQuality Compounder · /u);
  expect(markdown).toContain("## You · ");
  expect(markdown).toContain("## Quality Compounder · ");
  expect(markdown).toContain("Qualification costs rose. [^1]");
  expect(markdown).toContain("[^1]: Q3 operating review — https://hanul.example/q3");
  expect(markdown.trim().split("\n").at(-1)).toBe("_AI can be wrong. Not a licensed financial adviser._");
});

it("attributes the opening brief to Bot and quotes every line of the user's original message", () => {
  const at = "2026-11-19T08:00:00Z";
  const thread: ChatThread = { id: "research", loupist_id: "owner", title: "Chip margins", updated_at: at, use_memory: true, update_memory: true,
    messages: [{ id: "opening", role: "bot_handoff", content: "**Check margins.**", created_at: at, handoff_note: { title: "Chip margins", userMessageInBot: "Then go ahead.\nJust chips." } }] };
  const markdown = chatMarkdown(thread, "Quality");
  expect(markdown).toContain("## From Bot · ");
  expect(markdown).toContain("**Check margins.**\n\nYou said in Bot\n\n> Then go ahead.\n> Just chips.");
  expect(markdown).not.toContain("## You");
});

it("names a scheduled run's request by its task, and leaves the briefing's host request out", () => {
  const at = "2026-11-19T08:00:00Z";
  const thread = (kind: string, title: string, content: string): ChatThread => ({
    id: "scheduled-chat-1", loupist_id: "loupist", title, updated_at: at, use_memory: true, update_memory: true,
    messages: [
      { id: "q", role: "user", content, created_at: at, status: "complete", scheduled: { task_id: "task", run_id: "run", task_title: title, task_kind: kind } },
      { id: "a", role: "assistant", content: "Neither crossed today.", created_at: at, status: "complete" },
    ],
  });
  const custom = chatMarkdown(thread("custom", "Daily chip check", "Check the chip makers."), "Quality Compounder");
  expect(custom).toContain("## Scheduled run: Daily chip check · ");
  expect(custom).toContain("Check the chip makers.");
  expect(custom).not.toContain("## You");
  const briefing = chatMarkdown(thread("briefing", "Briefings", "This is a scheduled briefing. {\"followed\":[]}"), "Quality Compounder");
  expect(briefing).toContain("## Scheduled run: Briefing · ");
  expect(briefing).not.toContain("This is a scheduled briefing.");
  expect(briefing).toContain("Neither crossed today.");
});

it("footnotes each number link with its own check, by its place among the links Rust counted", () => {
  const evidence = { id: "e1", title: "Q3 operating review", kind: "original" as const, url: "https://hanul.example/q3", provider: "IR", tool_name: "web_fetch", retrieved_at: "2026-10-28T00:00:00Z", published_at: "2026-10-27", excerpt: "", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [] };
  const thread: ChatThread = {
    id: "chat", loupist_id: "loupist", title: "Margins", updated_at: "2026-10-28T00:00:00Z", use_memory: true, update_memory: true,
    messages: [{ id: "a", role: "assistant", status: "complete", created_at: "2026-10-28T00:00:12Z", evidence_refs: [evidence],
      // Code holds no link; of the two links, only the second has a check.
      content: "  Code `[0.5%](#evidence-e1)`; margin [0.5%](#evidence-e1), again [0.5%](#evidence-e1).",
      number_checks: [{ link_index: 1, text: "0.5%", evidence_id: "e1", kind: "passage", status: "matched" }] }],
  };
  const markdown = chatMarkdown(thread, "Quality Compounder", Date.parse("2026-10-28T00:00:00Z"));
  expect(markdown).toContain("Code `[0.5%](#evidence-e1)`; margin [0.5%](#evidence-e1), again 0.5%[^1].");
  expect(markdown).toContain("[^1]: Q3 operating review — 2026-10-27 — https://hanul.example/q3");
  expect(markdown).not.toContain("[^2]");
});

it("quotes CSV cells that need it and keeps file names safe", () => {
  expect(datasetCsv([{ label: "Name" }, { label: "Note" }], [["Hanul", 'Said "up", then down'], ["Morrow", null]]))
    .toBe('Name,Note\r\nHanul,"Said ""up"", then down"\r\nMorrow,');
  expect(exportFileName("../Q3: margin / review")).toBe("Q3 margin review");
  expect(exportFileName("///")).toBe("chat");
});

it("writes numbers with the digits the data has: a widened 32-bit float loses only its noise", () => {
  expect(String(Math.fround(225.3))).toBe("225.3000030517578");
  expect([Math.fround(225.3), Math.fround(0.0123), Math.fround(-12.34), Math.fround(98765.43)].map(datasetCellText)).toEqual(["225.3", "0.0123", "-12.34", "98765.43"]);
  // Real double-precision values, integers and short exact fractions stay as stored.
  expect([225.3, 1234.5678, 0.1 + 0.2, 1234567.25, 123_456_792, 394_328_000_000, 1e-7, 0.5, "1.50", null].map(datasetCellText))
    .toEqual(["225.3", "1234.5678", "0.30000000000000004", "1234567.25", "123456792", "394328000000", "1e-7", "0.5", "1.50", ""]);
  expect(datasetCsv([{ label: "Close" }], [[Math.fround(225.3)]])).toBe("Close\r\n225.3");
});
