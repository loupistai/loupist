/** The main Chat panel's composer, else the one composer on the page. */
export function chatComposer(): HTMLTextAreaElement | null {
  return document.querySelector<HTMLTextAreaElement>("#main-panel-chat textarea[data-chat-composer]")
    ?? document.querySelector<HTMLTextAreaElement>("textarea[data-chat-composer]");
}

/**
 * An action put words or a reference into the composer and hands it the keyboard: the caret goes
 * after everything in it, so the person types on. A scripted focus alone leaves the caret where the
 * field last had it, or at its start in WebKit, so the selection is set here, once the content is in
 * the DOM. A hidden or inert composer is left alone. Returns whether the composer has focus.
 */
export function focusComposerAtEnd(textarea = chatComposer()): boolean {
  if (!textarea || textarea.closest("[hidden], [inert]")) return false;
  textarea.focus({ preventScroll: true });
  if (document.activeElement !== textarea) return false;
  const end = textarea.value.length;
  textarea.setSelectionRange(end, end);
  return true;
}
