import { fromMarkdown } from "mdast-util-from-markdown";
import { workspaceFileLink } from "../lib/workspaceFileLink";

/** A file becomes a deliverable when the answer links it, not merely when a tool writes it. */
export function deliveredFiles(markdown: string, workspacePath?: string): Set<string> {
  const tree = fromMarkdown(markdown);
  type Node = { type: string; url?: string; identifier?: string; children?: Node[] };
  const definitions = new Map<string, string>();
  const collectDefinitions = (node: Node) => {
    if (node.type === "definition" && node.identifier && node.url) definitions.set(node.identifier, node.url);
    node.children?.forEach(collectDefinitions);
  };
  collectDefinitions(tree);
  const paths = new Set<string>();
  const visit = (node: Node) => {
    const href = node.type === "link" ? node.url
      : node.type === "linkReference" && node.identifier ? definitions.get(node.identifier) : undefined;
    const path = href ? workspaceFileLink(href, workspacePath) : null;
    if (path) paths.add(path);
    node.children?.forEach(visit);
  };
  visit(tree);
  return paths;
}
