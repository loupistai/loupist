import { composeSubmission, type ComposerChip } from "./composerChips";
import { rebaseInlineReferences, serializeInlineReferences, type ComposerInlineReference } from "./composerInlineReferences";

it("rebases an inline object when surrounding prose changes, and detaches it when its text is edited", () => {
  const reference: ComposerInlineReference = { key: "pin:source", kind: "source", text: "⟦Report⟧", start: 5, end: 13 };
  const shifted = rebaseInlineReferences("Read ⟦Report⟧ today", "Please read ⟦Report⟧ today", [reference]);
  expect(shifted).toEqual([{ ...reference, start: 12, end: 20 }]);
  expect(rebaseInlineReferences("Read ⟦Report⟧ today", "Read ⟦Revised⟧ today", [reference])).toEqual([]);
  expect(serializeInlineReferences("Read ⟦Revised⟧ today", [reference], () => "private-id")).toBe("Read ⟦Revised⟧ today");
});

it("serializes inline objects where they were selected and keeps whole-message Skills as modes", () => {
  const chips: ComposerChip[] = [
    { key: "skill:research", kind: "skill", id: "research", label: "Research" },
    { key: "file:report", kind: "file", id: "reports/q3.md", label: "q3.md" },
    { key: "quote:answer", kind: "quote", id: "answer", label: "quote", text: "Margin fell." },
  ];
  const prompt = "Compare @q3.md with “Margin fell.”";
  const references = [
    { key: "chip:file:report", kind: "file", text: "@q3.md", start: 8, end: 14 },
    { key: "chip:quote:answer", kind: "quote", text: "“Margin fell.”", start: 20, end: 34 },
  ];
  expect(composeSubmission(prompt, chips, references)).toBe("$research\n\nCompare reports/q3.md with “Margin fell.”");
});
