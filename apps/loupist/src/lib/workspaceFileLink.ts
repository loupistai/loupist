const normalizeFilesystemPath = (value: string) => {
  const path = value.replace(/\\/g, "/");
  if (/^\/\/\?\/UNC\//i.test(path)) return `//${path.slice(8)}`;
  if (/^\/\/\?\/[a-z]:\//i.test(path)) return path.slice(4);
  return path;
};
const isWindowsAbsolute = (value: string) => /^[a-z]:\//i.test(value) || /^\/\/[^/]+\/[^/]+(?:\/|$)/.test(value);
const asciiLower = (value: string) => value.replace(/[A-Z]/g, (letter) => letter.toLowerCase());

/** Convert a displayed file link to a workspace-relative path. Rust checks the actual filesystem. */
export function workspaceFileLink(href: string, rootPath?: string, basePath = ""): string | null {
  let value: string;
  try { value = decodeURIComponent(href); } catch { return null; }
  value = normalizeFilesystemPath(value);
  if (!value || /[\u0000-\u001f\u007f?]/u.test(value) || value.startsWith("#")) return null;
  value = value.replace(/#L\d+(?:-L?\d+)?$/, "").replace(/:\d+(?:-\d+)?$/, "");
  if (value.includes("#")) return null;
  const root = rootPath ? normalizeFilesystemPath(rootPath).replace(/\/+$/, "") || "/" : undefined;
  const windowsPath = isWindowsAbsolute(value);
  if (/^[a-z][a-z\d+.-]*:/i.test(value) && !windowsPath) return null;
  let segments = basePath.split("/").filter(Boolean);
  if (value.startsWith("/") || windowsPath) {
    if (!root) return null;
    const windowsRoot = isWindowsAbsolute(`${root}/`);
    if (windowsPath !== windowsRoot) return null;
    const prefix = root.endsWith("/") ? root : `${root}/`;
    const matchesRoot = windowsRoot ? asciiLower(value).startsWith(asciiLower(prefix)) : value.startsWith(prefix);
    if (!matchesRoot) return null;
    value = value.slice(prefix.length);
    segments = [];
  }
  for (const part of value.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") { if (!segments.length) return null; segments.pop(); }
    else segments.push(part);
  }
  return segments.length ? segments.join("/") : null;
}

export const WORKSPACE_LINK_PROTOCOL = "loupist-workspace";
const WORKSPACE_LINK_PREFIX = `${WORKSPACE_LINK_PROTOCOL}:`;
type LinkNode = { type?: string; value?: string; tagName?: string; properties?: { href?: unknown; src?: unknown }; children?: LinkNode[] };

/** Normalize workspace links and image sources before the protocol sanitizer. */
export function workspaceLinksPlugin({ rootPath, basePath }: { rootPath?: string; basePath?: string } = {}) {
  return (tree: LinkNode) => {
    // Models sometimes omit CommonMark's angle brackets around filenames with
    // spaces. Recover only plain-text file links; never reinterpret code,
    // images, web URLs, or a path outside the selected workspace.
    const recoverFileLinks = (node: LinkNode): LinkNode[] => {
      if (node.type !== "text" || !node.value) return [node];
      const text = node.value;
      const pattern = /(?<!!)\[([^\[\]\n]+)\]\(([^()<>"\n]+)\)/gu;
      const result: LinkNode[] = [];
      let start = 0;
      for (const match of text.matchAll(pattern)) {
        const href = match[2];
        if (!href.includes(" ") || !/\.[a-z\d]{1,12}(?:#L\d+(?:-L?\d+)?)?$/iu.test(href)) continue;
        const path = workspaceFileLink(href, rootPath, basePath);
        if (!path) continue;
        result.push({ type: "text", value: text.slice(start, match.index) }, {
          type: "element", tagName: "a", properties: { href: `${WORKSPACE_LINK_PREFIX}${encodeURIComponent(path)}` },
          children: [{ type: "text", value: match[1] }],
        });
        start = match.index + match[0].length;
      }
      return start ? [...result, { type: "text", value: text.slice(start) }] : [node];
    };
    const visit = (node: LinkNode) => {
      if (node.tagName === "code" || node.tagName === "pre") return;
      if (node.tagName === "img" && typeof node.properties?.src === "string") {
        const path = workspaceFileLink(node.properties.src, rootPath, basePath);
        if (path) node.properties.src = `${WORKSPACE_LINK_PREFIX}${encodeURIComponent(path)}`;
        return;
      }
      if (node.tagName === "a" && typeof node.properties?.href === "string") {
        const path = workspaceFileLink(node.properties.href, rootPath, basePath);
        if (path) node.properties.href = `${WORKSPACE_LINK_PREFIX}${encodeURIComponent(path)}`;
        return;
      }
      if (node.children) node.children = node.children.flatMap(recoverFileLinks);
      node.children?.forEach(visit);
    };
    visit(tree);
  };
}

export function decodedWorkspaceLink(href: string): string | null {
  return href.startsWith(WORKSPACE_LINK_PREFIX) ? workspaceFileLink(href.slice(WORKSPACE_LINK_PREFIX.length)) : null;
}
