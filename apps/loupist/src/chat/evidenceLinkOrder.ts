import { fromMarkdown } from "mdast-util-from-markdown";

/**
 * An evidence target as Rust reads it (`number_checks.rs::evidence_id`), after it
 * restores a target written with its result-field name (`citation_url=#evidence-…`).
 */
const EVIDENCE_TARGET = /^(?:citation_url[=:])?#evidence-[A-Za-z0-9_]{1,128}$/u;

type Node = { type: string; url?: string; identifier?: string; children?: Node[]; position?: { start: { offset?: number } } };

/**
 * Where each evidence link of this Markdown starts (its `[`), in reading order.
 * Rust numbers the same links when it checks a finished answer: a check's
 * `link_index` n is the link at the n-th start here. Links in code are not
 * links; a reference link counts where it is written.
 */
export function evidenceLinkStarts(markdown: string): number[] {
  const tree = fromMarkdown(markdown) as Node;
  const definitions = new Map<string, string>();
  const define = (node: Node) => {
    // The first definition of a label is the one CommonMark uses.
    if (node.type === "definition" && node.identifier && node.url !== undefined && !definitions.has(node.identifier)) definitions.set(node.identifier, node.url);
    node.children?.forEach(define);
  };
  define(tree);
  const starts: number[] = [];
  const visit = (node: Node) => {
    const target = node.type === "link" ? node.url
      : node.type === "linkReference" && node.identifier ? definitions.get(node.identifier) : undefined;
    const start = node.position?.start.offset;
    if (target && EVIDENCE_TARGET.test(target) && start !== undefined) starts.push(start);
    node.children?.forEach(visit);
  };
  visit(tree);
  return starts;
}
