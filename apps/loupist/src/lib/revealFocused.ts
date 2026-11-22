/** WebKit leaves a focused control past the edge of an overflow port. Reveal it. */
export function revealFocused(event: { target: EventTarget | null }) {
  const target = event.target;
  if (target instanceof HTMLElement && focusWouldScroll(target)) target.scrollIntoView({ block: "nearest", inline: "nearest" });
}

/** Returning focus from a panel must leave the user's reading position intact. */
export function focusWouldScroll(target: HTMLElement) {
  const rect = target.getBoundingClientRect();
  if (rect.width < 1 || rect.height < 1) return false;
  const inset = 8;
  let node = target.parentElement;
  while (node && node !== document.body) {
    const overflow = getComputedStyle(node).overflowY;
    if ((overflow === "auto" || overflow === "scroll") && node.scrollHeight > node.clientHeight + 1) {
      const box = node.getBoundingClientRect();
      if (rect.top < box.top + inset || rect.bottom > box.bottom - inset || rect.left < box.left || rect.right > box.right) return true;
    }
    node = node.parentElement;
  }
  return false;
}
