import { focusComposerAtEnd } from "./focusComposer";

afterEach(() => { document.body.innerHTML = ""; vi.restoreAllMocks(); });

/** WebKit puts the caret at a field's start on a scripted focus when it has no remembered place. */
function webKitFocus() {
  const focus = HTMLTextAreaElement.prototype.focus;
  vi.spyOn(HTMLTextAreaElement.prototype, "focus").mockImplementation(function (this: HTMLTextAreaElement, options?: FocusOptions) {
    focus.call(this, options);
    this.setSelectionRange(0, 0);
  });
}

function composer(value: string, panel = "") {
  document.body.innerHTML = `<div id="main-panel-chat" ${panel}><textarea data-chat-composer="true"></textarea></div>`;
  const textarea = document.querySelector("textarea")!;
  textarea.value = value;
  return textarea;
}

it("focuses the composer with the caret after everything an action put in it", () => {
  webKitFocus();
  const textarea = composer("⟦Price and volume⟧ What changed?");
  expect(focusComposerAtEnd()).toBe(true);
  expect(textarea).toHaveFocus();
  expect([textarea.selectionStart, textarea.selectionEnd]).toEqual([textarea.value.length, textarea.value.length]);
});

it("leaves a composer hidden behind a full page alone", () => {
  const textarea = composer("Question", "hidden");
  expect(focusComposerAtEnd()).toBe(false);
  expect(textarea).not.toHaveFocus();
});
