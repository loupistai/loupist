import { chatMarkdown, datasetCsv, exportFileName } from "./exportChat";
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

it("quotes CSV cells that need it and keeps file names safe", () => {
  expect(datasetCsv([{ label: "Name" }, { label: "Note" }], [["Hanul", 'Said "up", then down'], ["Morrow", null]]))
    .toBe('Name,Note\r\nHanul,"Said ""up"", then down"\r\nMorrow,');
  expect(exportFileName("../Q3: margin / review")).toBe("Q3 margin review");
  expect(exportFileName("///")).toBe("chat");
});
