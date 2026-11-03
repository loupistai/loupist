import { useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import type { ChatDraft } from "../../app/useChatDrafts";
import { rebaseInlineReferences, validInlineReferences, type ComposerInlineReference } from "../../chat/composerInlineReferences";

export type InlineComposerItem = { key: string; kind: string; text: string; label: string; onRemove: () => void; onRestore?: () => void };
type EditorSnapshot = { prompt: string; references: ComposerInlineReference[]; items: InlineComposerItem[]; start: number; end: number };

/** Keep native textarea selection/IME behavior, with bounded typed references anchored to visible text. */
export function useComposerInlineReferences({ prompt, onPromptChange, items, draft, textarea }: {
  prompt: string; onPromptChange: (prompt: string) => void; items: InlineComposerItem[]; draft?: ChatDraft;
  textarea: { current: HTMLTextAreaElement | null };
}) {
  const [references, setReferences] = useState<ComposerInlineReference[]>(() => validInlineReferences(prompt, draft?.inlineReferences ?? []));
  const live = useRef({ prompt, items, onPromptChange, draft });
  live.current = { prompt, items, onPromptChange, draft };
  const refs = useRef(references);
  const previous = useRef(prompt);
  const handled = useRef(new Set(references.map(reference => reference.key)));
  const selection = useRef({ start: prompt.length, end: prompt.length });
  const pendingCursor = useRef<{ cursor: number; focus: boolean } | null>(null);
  const history = useRef<{ past: EditorSnapshot[]; future: EditorSnapshot[]; action: string; at: number }>({ past: [], future: [], action: "", at: 0 });
  const itemKey = items.map(item => item.key).join("\n");
  const save = (next: ComposerInlineReference[]) => {
    refs.current = next;
    if (live.current.draft) live.current.draft.inlineReferences = next;
    setReferences(next);
  };
  const detach = (removed: readonly ComposerInlineReference[], kept: readonly ComposerInlineReference[]) => {
    const retained = new Set(kept.map(reference => reference.key));
    const removedKeys = new Set(removed.map(reference => reference.key));
    for (const key of removedKeys) if (!retained.has(key)) live.current.items.find(item => item.key === key)?.onRemove();
  };
  const rememberSelection = (element: HTMLTextAreaElement) => {
    selection.current = { start: element.selectionStart, end: element.selectionEnd };
  };
  const restoreCursor = (cursor: number, focus: boolean) => {
    selection.current = { start: cursor, end: cursor };
    pendingCursor.current = { cursor, focus };
  };
  const snapshot = (text: string, ranges: readonly ComposerInlineReference[], known = live.current.items): EditorSnapshot => ({
    prompt: text, references: [...ranges], items: known.filter(item => ranges.some(reference => reference.key === item.key)),
    start: Math.min(selection.current.start, text.length), end: Math.min(selection.current.end, text.length),
  });
  const record = (action: string, next: string) => {
    if (next === previous.current) return;
    const stack = history.current;
    const now = performance.now();
    const continuousTyping = action === "typing" && stack.action === "typing" && now - stack.at < 750;
    if (!continuousTyping) stack.past.push(snapshot(previous.current, refs.current));
    // Keep undo bounded independently from native persisted draft state.
    while (stack.past.length > 50 || stack.past.reduce((size, entry) => size + entry.prompt.length, 0) > 1_000_000) stack.past.shift();
    stack.future = [];
    stack.action = action;
    stack.at = now;
  };
  const undo = (redo = false) => {
    const stack = history.current;
    const source = redo ? stack.future : stack.past;
    const target = source.pop();
    if (!target) return;
    (redo ? stack.past : stack.future).push(snapshot(previous.current, refs.current));
    stack.action = "history";
    const targetKeys = new Set(target.items.map(item => item.key));
    const currentKeys = new Set(live.current.items.map(item => item.key));
    for (const item of live.current.items) if (!targetKeys.has(item.key)) item.onRemove();
    for (const item of target.items) if (!currentKeys.has(item.key)) item.onRestore?.();
    previous.current = target.prompt;
    handled.current = new Set(target.items.map(item => item.key));
    save(target.references);
    live.current.onPromptChange(target.prompt);
    selection.current = { start: target.start, end: target.end };
    pendingCursor.current = { cursor: target.end, focus: true };
  };
  const edit = (next: string) => {
    record("typing", next);
    const rebased = rebaseInlineReferences(previous.current, next, refs.current);
    const kept = new Set(rebased.map(reference => reference.key));
    detach(refs.current.filter(reference => !rebased.includes(reference) && !kept.has(reference.key)), rebased);
    previous.current = next;
    save(rebased);
    live.current.onPromptChange(next);
  };
  const insert = (item: InlineComposerItem, start = selection.current.start, end = selection.current.end, focus = true) => {
    const source = previous.current;
    start = Math.max(0, Math.min(source.length, start));
    end = Math.max(start, Math.min(source.length, end));
    const before = source.slice(0, start), after = source.slice(end);
    const leading = before && !/\s$/u.test(before) ? " " : "";
    const trailing = !after || !/^\s/u.test(after) ? " " : "";
    const next = `${before}${leading}${item.text}${trailing}${after}`;
    record("reference", next);
    const rebased = rebaseInlineReferences(source, next, refs.current);
    const kept = new Set(rebased.map(reference => reference.key));
    detach(refs.current.filter(reference => reference.key !== item.key && !kept.has(reference.key)), rebased);
    const offset = start + leading.length;
    handled.current.add(item.key);
    previous.current = next;
    save([...rebased, { key: item.key, kind: item.kind, text: item.text, start: offset, end: offset + item.text.length }].sort((a, b) => a.start - b.start));
    live.current.onPromptChange(next);
    restoreCursor(offset + item.text.length + trailing.length, focus);
  };
  const remove = (key: string, occurrenceStart?: number) => {
    const targets = refs.current.filter(reference => reference.key === key && (occurrenceStart === undefined || reference.start === occurrenceStart));
    const item = live.current.items.find(item => item.key === key);
    record("reference", targets.length ? previous.current.slice(0, targets[0]!.start) + previous.current.slice(targets[0]!.end) : previous.current);
    handled.current.add(key);
    if (!refs.current.some(reference => reference.key === key && !targets.includes(reference))) item?.onRemove();
    if (!targets.length) return;
    let next = previous.current;
    let rebased = refs.current;
    for (const reference of [...targets].sort((a, b) => b.start - a.start)) {
      const removed = next.slice(0, reference.start) + next.slice(reference.end);
      rebased = rebaseInlineReferences(next, removed, rebased.filter(item => item.key !== reference.key || item.start !== reference.start));
      next = removed;
    }
    previous.current = next;
    save(rebased);
    live.current.onPromptChange(next);
    restoreCursor(Math.min(...targets.map(reference => reference.start)), true);
  };
  const keyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    const key = event.key.toLowerCase();
    if ((event.metaKey || event.ctrlKey) && !event.altKey && (key === "z" || key === "y")) {
      event.preventDefault();
      undo(key === "y" || event.shiftKey);
      return true;
    }
    if (event.key !== "Backspace" && event.key !== "Delete") return false;
    const { selectionStart: start, selectionEnd: end } = event.currentTarget;
    if (start !== end) return false;
    const reference = refs.current.find(reference => event.key === "Backspace"
      ? reference.start < start && start <= reference.end : reference.start <= start && start < reference.end);
    if (!reference) return false;
    event.preventDefault();
    // Keyboard editing removes only this occurrence; the named context action removes all of them.
    remove(reference.key, reference.start);
    return true;
  };

  useLayoutEffect(() => {
    const current = live.current;
    // ChatView adopts an externally restored draft in its effect. Wait for its controlled textarea
    // value before anchoring restored objects, otherwise the old local value could overwrite it.
    if (current.draft && current.draft.prompt !== current.prompt) return;
    const active = new Set(current.items.map(item => item.key));
    const restored = current.draft?.inlineReferences;
    if (restored && restored !== refs.current) {
      const valid = validInlineReferences(current.prompt, restored).filter(reference => active.has(reference.key));
      if (valid.length) {
        history.current = { past: [], future: [], action: "", at: 0 };
        previous.current = current.prompt;
        for (const reference of valid) handled.current.add(reference.key);
        save(valid);
      }
    }
    for (const key of handled.current) if (!active.has(key)) handled.current.delete(key);
    if (current.prompt !== previous.current) {
      history.current = { past: [], future: [], action: "", at: 0 };
      // External draft replacements (including accepted sends) must not reinsert consumed context.
      const rebased = rebaseInlineReferences(previous.current, current.prompt, refs.current);
      const kept = new Set(rebased.map(reference => reference.key));
      detach(refs.current.filter(reference => active.has(reference.key) && !kept.has(reference.key)), rebased);
      previous.current = current.prompt;
      save(rebased.filter(reference => active.has(reference.key)));
      selection.current.start = Math.min(selection.current.start, current.prompt.length);
      selection.current.end = Math.min(selection.current.end, current.prompt.length);
    }
    // Replacing or removing context from another surface also removes its visible token.
    const dropped = refs.current.filter(reference => !active.has(reference.key)).sort((a, b) => b.start - a.start);
    for (const reference of dropped) {
      const next = previous.current.slice(0, reference.start) + previous.current.slice(reference.end);
      const rebased = rebaseInlineReferences(previous.current, next, refs.current.filter(item => item.key !== reference.key));
      previous.current = next;
      const shift = (position: number) => position <= reference.start ? position : Math.max(reference.start, position - (reference.end - reference.start));
      selection.current = { start: shift(selection.current.start), end: shift(selection.current.end) };
      save(rebased);
      current.onPromptChange(next);
    }
    const missing = current.items.filter(item => !handled.current.has(item.key));
    for (const item of missing) insert(item, selection.current.start, selection.current.end, false);
    // Commit before paint, after React writes the controlled value. A delayed animation frame could
    // otherwise move the caret back while the person is already typing their next word.
    const pending = pendingCursor.current;
    if (pending && textarea.current?.value === previous.current) {
      pendingCursor.current = null;
      if (pending.focus) textarea.current.focus();
      textarea.current.setSelectionRange(pending.cursor, pending.cursor);
    }
  }, [itemKey, prompt, draft?.prompt, draft?.inlineReferences]);
  useLayoutEffect(() => {
    const element = textarea.current;
    if (!element) return;
    const beforeInput = (event: InputEvent) => {
      if (event.isComposing || (event.inputType !== "historyUndo" && event.inputType !== "historyRedo")) return;
      event.preventDefault();
      undo(event.inputType === "historyRedo");
    };
    element.addEventListener("beforeinput", beforeInput);
    return () => element.removeEventListener("beforeinput", beforeInput);
  }, []);
  return { references, insert, edit, remove, keyDown, rememberSelection };
}
