import { useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChatComposer } from "./ChatComposer";
import { TooltipProvider } from "../ui/tooltip";
import type { ChatDraft } from "../../app/useChatDrafts";
import { quoteChip, type ComposerChip } from "../../chat/composerChips";
import type { KnowledgePin } from "../../types";

const pin: KnowledgePin = { kind: "world_node", loupistId: "loupist", objectId: "entity-1", label: "Apple" };
const makeDraft = (prompt: string): ChatDraft => ({ prompt, pins: [], files: [], chips: [] });

function Composer({ draft, onSend = vi.fn() }: { draft: ChatDraft; onSend?: (text: string, pins: KnowledgePin[]) => void }) {
  const [prompt, setPrompt] = useState(draft.prompt);
  const [chips, setChips] = useState<ComposerChip[]>(draft.chips ?? []);
  const [pins, setPins] = useState<KnowledgePin[]>(draft.pins);
  return <TooltipProvider>
    <button onClick={() => { draft.pins = [pin]; setPins([pin]); }}>Add object</button>
    <button onClick={() => { const next = [...chips, quoteChip("answer", "Margin fell last year.")]; draft.chips = next; setChips(next); }}>Add quote</button>
    <ChatComposer attachmentDraft={draft} prompt={prompt} onPromptChange={value => { draft.prompt = value; setPrompt(value); }}
      privateChat isRunning={false} error={null} streamAnnouncement="" asOf="2026-03-03" onAsOfChange={vi.fn()}
      models={[{ id: "model", provider: "test", model: "model", name: "Model", input: ["text"], reasoning: true, context_window: 10000, max_tokens: 1000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" }]} providers={[]} modelSelection={{ model_profile_id: "model", thinking_level: "medium", run_options: {} }} modelAuto onModelResetToAuto={() => {}}
      skills={[{ id: "research", name: "Research", description: "Research a topic" }]} plugins={[]}
      chips={chips} onChipsChange={next => { draft.chips = next; setChips(next); }} knowledgePins={pins}
      onAddKnowledgePin={restored => { const next = [...draft.pins.filter(item => item.objectId !== restored.objectId), restored]; draft.pins = next; setPins(next); }}
      onRemoveKnowledgePin={removed => { const next = draft.pins.filter(item => item.objectId !== removed.objectId); draft.pins = next; setPins(next); }}
      onSearchContext={async () => [{ id: "file:report", group: "Files", label: "report.md", detail: "Quarterly report", path: "reports/report.md" }]}
      onPrivateChatChange={vi.fn()} onModelSelectionChange={vi.fn()} onSend={text => { onSend(text, pins); return false; }} onFollowUp={vi.fn()} onAbort={vi.fn()}
    />
  </TooltipProvider>;
}

it("inserts a selected file at the cursor, preserves trailing prose and sends its path in place", async () => {
  const user = userEvent.setup(), send = vi.fn();
  const draft = makeDraft("Compare  with last year");
  render(<Composer draft={draft} onSend={send} />);
  const input = screen.getByRole("textbox", { name: "Message Loupist" }) as HTMLTextAreaElement;
  input.focus(); input.setSelectionRange(8, 8); fireEvent.select(input);
  await user.type(input, "@report", { skipClick: true });
  await user.click(await screen.findByRole("option", { name: "report.md" }));
  expect(input).toHaveValue("Compare @report.md with last year");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(send).toHaveBeenCalledWith("Compare reports/report.md with last year", []);
  const status = screen.getByText("Private · memory off");
  expect(input.compareDocumentPosition(status) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

it("keeps an object typed, removes it atomically and leaves the rest of the question editable", async () => {
  const user = userEvent.setup(), send = vi.fn(), draft = makeDraft("Read  carefully");
  render(<Composer draft={draft} onSend={send} />);
  const input = screen.getByRole("textbox", { name: "Message Loupist" }) as HTMLTextAreaElement;
  input.focus(); input.setSelectionRange(5, 5); fireEvent.select(input);
  await user.click(screen.getByRole("button", { name: "Add object" }));
  expect(input).toHaveValue("Read ⟦Apple⟧ carefully");
  expect(draft.pins).toEqual([pin]);
  const reference = draft.inlineReferences![0]!;
  input.focus(); input.setSelectionRange(reference.end, reference.end);
  await user.keyboard("{Backspace}");
  expect(input).toHaveValue("Read  carefully");
  expect(draft.pins).toEqual([]);
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(send).toHaveBeenCalledWith("Read  carefully", []);
});

it("restores quote ranges with a draft and detaches edited references without disturbing IME input", async () => {
  const user = userEvent.setup(), draft = makeDraft("Explain ");
  render(<Composer draft={draft} />);
  await user.click(screen.getByRole("button", { name: "Add quote" }));
  expect(draft.prompt).toBe("Explain “Margin fell last year.” ");
  cleanup();
  render(<Composer draft={draft} />);
  const input = screen.getByRole("textbox", { name: "Message Loupist" }) as HTMLTextAreaElement;
  expect(input).toHaveValue("Explain “Margin fell last year.” ");
  expect(document.querySelectorAll("[data-inline-reference=quote]")).toHaveLength(1);
  fireEvent.compositionStart(input);
  fireEvent.change(input, { target: { value: "Explain “Margin grew last year.” ", selectionStart: 20 } });
  fireEvent.keyDown(input, { key: "Enter", keyCode: 229, isComposing: true });
  fireEvent.compositionEnd(input);
  expect(input).toHaveValue("Explain “Margin grew last year.” ");
  await waitFor(() => expect(draft.chips).toEqual([]));
  expect(draft.inlineReferences).toEqual([]);
});

it("serializes repeated references in place, deletes one occurrence at the caret and removes all through the named context action", async () => {
  const user = userEvent.setup(), send = vi.fn(), draft = makeDraft("Compare ");
  render(<Composer draft={draft} onSend={send} />);
  const input = screen.getByRole("textbox", { name: "Message Loupist" }) as HTMLTextAreaElement;
  const pick = async (text: string) => {
    await user.type(input, text, { skipClick: true });
    await user.click(await screen.findByRole("option", { name: "report.md" }));
  };
  input.focus(); input.setSelectionRange(input.value.length, input.value.length);
  await pick("@report");
  await pick("and @report");
  expect(draft.inlineReferences).toHaveLength(2);
  expect(document.querySelectorAll('[data-inline-reference="file"]')).toHaveLength(2);
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(send).toHaveBeenLastCalledWith("Compare reports/report.md and reports/report.md", []);

  const first = draft.inlineReferences![0]!;
  input.focus(); input.setSelectionRange(first.end, first.end);
  await user.keyboard("{Backspace}");
  expect(draft.inlineReferences).toHaveLength(1);
  expect(draft.chips).toHaveLength(1);
  expect(input.value.match(/@report/gu)).toHaveLength(1);
  await pick("@report");
  expect(draft.inlineReferences).toHaveLength(2);
  await user.click(screen.getByRole("button", { name: "Remove report.md" }));
  expect(draft.inlineReferences).toEqual([]);
  expect(draft.chips).toEqual([]);
  expect(input.value).not.toContain("report.md");
  expect(input.value).toContain("Compare");
  expect(input.value).toContain("and");
});

it("undoes and redoes reference changes together with typed context, without losing surrounding Korean prose", async () => {
  const user = userEvent.setup(), send = vi.fn(), draft = makeDraft("이 자료를 검토해줘 ");
  render(<Composer draft={draft} onSend={send} />);
  const input = screen.getByRole("textbox", { name: "Message Loupist" }) as HTMLTextAreaElement;
  await user.click(screen.getByRole("button", { name: "Add quote" }));
  expect(draft.chips).toHaveLength(1);
  input.focus();
  await user.keyboard("{Meta>}z{/Meta}");
  expect(input).toHaveValue("이 자료를 검토해줘 ");
  expect(draft.chips).toEqual([]);
  await user.keyboard("{Meta>}{Shift>}z{/Shift}{/Meta}");
  expect(input).toHaveValue("이 자료를 검토해줘 “Margin fell last year.” ");
  expect(draft.chips).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(send).toHaveBeenLastCalledWith("이 자료를 검토해줘 “Margin fell last year.”", []);

  await user.click(screen.getByRole("button", { name: "Add object" }));
  const object = draft.inlineReferences!.find(reference => reference.kind === "world_node")!;
  input.focus(); input.setSelectionRange(object.end, object.end);
  await user.keyboard("{Backspace}");
  expect(draft.pins).toEqual([]);
  await user.keyboard("{Meta>}z{/Meta}");
  expect(draft.pins).toEqual([pin]);
  expect(input.value).toContain("⟦Apple⟧");
});
