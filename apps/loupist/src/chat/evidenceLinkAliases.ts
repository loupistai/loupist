import type { Element, Root, RootContent } from "hast";

/**
 * A tool result names its evidence `"citation_url": "#evidence-…"`, and a model
 * sometimes copies the field name into the link: `[$162.85](citation_url=#evidence-…)`.
 * That target is the evidence link it meant; anything else is left for the sanitizer.
 */
const ALIAS = /^citation_url[=:](#evidence-[A-Za-z0-9_]{1,128})$/u;

function rewrite(node: Root | RootContent) {
  if (node.type !== "root" && node.type !== "element") return;
  const element = node as Element;
  if (node.type === "element" && (element.tagName === "code" || element.tagName === "pre")) return;
  if (node.type === "element" && element.tagName === "a" && typeof element.properties.href === "string") {
    const evidence = element.properties.href.match(ALIAS)?.[1];
    if (evidence) element.properties.href = evidence;
  }
  node.children.forEach(rewrite);
}

/** Rehype plugin, before the sanitizer: restores the evidence link a model wrote with its field name. */
export function evidenceLinkAliases() {
  return (tree: Root) => rewrite(tree);
}
