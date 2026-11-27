import type { Element, ElementContent, Root, Text } from "hast";

/**
 * What belongs to the link it follows: a Korean particle or ending ("8.4%였고") and
 * closing punctuation ("[price quote](#evidence-…).", "…)", "。"), in any order.
 */
const TRAILING = /^(?:\p{Script=Hangul}|\p{Pe}|\p{Pf}|[.,;:!?…、。，．：；！？])+/u;
/** An opening bracket or quote right before a link: it never ends a line apart from it. */
const LEADING = /(?:\p{Ps}|\p{Pi})+$/u;

const linkHref = (node: Element) => String(node.properties?.href ?? "");

const isEvidenceLink = (node: ElementContent): node is Element =>
  node.type === "element" && node.tagName === "a" && linkHref(node).startsWith("#evidence-");

/** A lens: link is inline prose too: the period after it stays on its line. */
const keepsTrailingPunctuation = (node: ElementContent): node is Element => {
  if (node.type !== "element" || node.tagName !== "a") return false;
  const href = linkHref(node);
  return href.startsWith("#evidence-") || href.startsWith("lens:");
};

/** A numbered citation ("[3]"), as opposed to a number chip whose text is the figure. */
const isCitation = (node: Element) =>
  node.children.length === 1 && node.children[0]!.type === "text" && /^\d{1,3}$/u.test(node.children[0]!.value);

const text = (value: string): Text => ({ type: "text", value });

/** Move the trailing word, including emphasis, punctuation and its original spacing,
 * into the first citation's group. Stop at another chip or a block boundary. */
function takeCitationLead(nodes: ElementContent[], end: number): ElementContent[] {
  const lead: ElementContent[] = [];
  while (end > 0) {
    const node = nodes[end - 1]!;
    if (node.type === "text") {
      const tail = node.value.match(/\S+\s*$|\s+$/u)?.[0];
      if (!tail) break;
      node.value = node.value.slice(0, -tail.length);
      if (!node.value) nodes.splice(--end, 1);
      lead.unshift(text(tail));
      if (/[^\s\p{P}]/u.test(tail)) break;
    } else if (node.type === "element" && ["strong", "em", "del"].includes(node.tagName)) {
      const tail = takeCitationLead(node.children, node.children.length);
      if (!tail.length) break;
      lead.unshift({ ...node, children: tail });
      if (!node.children.length) nodes.splice(--end, 1);
      break;
    } else {
      // Keep an interactive word intact. Never split a link into duplicate controls.
      if (node.type === "element" && (node.tagName === "a" || node.tagName === "code" ||
          (node.tagName === "span" && node.properties.className?.toString() === "number-keep" &&
            node.children.some(part => isEvidenceLink(part) && !isCitation(part))))) {
        lead.unshift(node);
        nodes.splice(end - 1, 1);
      }
      break;
    }
  }
  return lead;
}

function keep(parent: Root | Element) {
  const children = parent.children as ElementContent[];
  for (let index = 0; index < children.length; index += 1) {
    const child = children[index]!;
    if (child.type !== "element") continue;
    if (!keepsTrailingPunctuation(child)) {
      keep(child);
      continue;
    }
    const previous = children[index - 1];
    const next = children[index + 1];
    const opening = previous?.type === "text" ? previous.value.match(LEADING)?.[0] : undefined;
    const closing = next?.type === "text" ? next.value.match(TRAILING)?.[0] : undefined;
    // Every evidence link has its group, so the period that streams in after it joins the group
    // and the chip keeps its place in the tree: it is never mounted again, open card and all.
    const group: Element = { type: "element", tagName: "span", properties: { className: ["number-keep"] },
      children: [...(opening ? [text(opening)] : []), child, ...(closing ? [text(closing)] : [])] };
    let start = index;
    let count = 1;
    const after: Text[] = [];
    if (closing && next?.type === "text") {
      count += 1;
      const rest = next.value.slice(closing.length);
      if (rest) after.push(text(rest));
    }
    if (opening && previous?.type === "text") {
      previous.value = previous.value.slice(0, -opening.length);
      if (!previous.value) { start -= 1; count += 1; }
    }
    children.splice(start, count, group, ...after);
    if (isCitation(child)) {
      const lead = takeCitationLead(children, start);
      if (lead.length) {
        start = children.indexOf(group);
        // A space between consecutive chips remains an ordinary break opportunity.
        if (lead.every(part => part.type === "text" && !part.value.trim())) {
          children.splice(start, 0, ...lead);
          start += lead.length;
        } else {
          children[start] = { type: "element", tagName: "span", properties: { className: ["citation-keep"] }, children: [...lead, group] };
        }
      }
    }
    index = start;
  }
}

/**
 * A rehype step for answers: what follows a number or citation stays on its line.
 * A number chip or citation is an inline button, so the browser may otherwise break
 * the line between "8.4%" and "였고", or leave the sentence's period alone at the start
 * of the next line. A numbered citation also keeps the word before it, and an opening
 * bracket stays with the link it opens.
 */
export function keepNumberParticles() {
  return (tree: Root) => keep(tree);
}
