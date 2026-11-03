import type { Element, ElementContent, Root } from "hast";

/** Hangul right after a link: a particle or ending that belongs to the linked number ("8.4%였고"). */
const PARTICLE = /^\p{Script=Hangul}+/u;

const isEvidenceLink = (node: ElementContent): node is Element =>
  node.type === "element" && node.tagName === "a" && String(node.properties?.href ?? "").startsWith("#evidence-");

function keep(parent: Root | Element) {
  const children = parent.children as ElementContent[];
  for (let index = 0; index < children.length; index += 1) {
    const child = children[index]!;
    if (child.type !== "element") continue;
    const next = children[index + 1];
    const particle = isEvidenceLink(child) && next?.type === "text" ? next.value.match(PARTICLE)?.[0] : undefined;
    if (!particle || next?.type !== "text") {
      keep(child);
      continue;
    }
    const rest = next.value.slice(particle.length);
    const group: Element = { type: "element", tagName: "span", properties: { className: ["number-keep"] }, children: [child, { type: "text", value: particle }] };
    children.splice(index, 2, group, ...(rest ? [{ type: "text" as const, value: rest }] : []));
  }
}

/**
 * A rehype step for answers: a Korean particle stays on the same line as the
 * number or citation it follows. A number chip is an inline button, so the
 * browser may otherwise break the line between "8.4%" and "였고".
 */
export function keepNumberParticles() {
  return (tree: Root) => keep(tree);
}
