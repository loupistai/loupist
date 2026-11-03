// Serialized into the capture page; keep this function independent of module scope.
export function markdownIsRendered(root = document) {
  // SafeMessageResponse removes Markdown markers from its lazy fallback, so
  // readable text alone cannot prove the renderer has mounted.
  if (root.querySelector(".message-response-fallback")) return false;
  return [...root.querySelectorAll(".message .message-content")].every(node =>
    !node.textContent.includes("**") && !node.textContent.includes("](#evidence-"));
}
