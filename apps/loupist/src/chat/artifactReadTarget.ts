import type { ChatEvidenceRef } from "../types";

/** Read only the host's leading metadata object, even when the saved body was truncated. */
export function artifactReadTarget(evidence: ChatEvidenceRef): string | null {
  if (evidence.kind !== "derived" || evidence.tool_name !== "artifact_read") return null;
  const text = evidence.excerpt.slice(0, 8192);
  const prefix = /^\s*\{\s*"artifact"\s*:\s*\{/u.exec(text);
  if (!prefix) return null;
  const start = prefix[0].length - 1;
  let depth = 0, quoted = false, escaped = false;
  for (let index = start; index < text.length; index++) {
    const char = text[index];
    if (quoted) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') quoted = false;
    } else if (char === '"') quoted = true;
    else if (char === "{") depth++;
    else if (char === "}" && --depth === 0) {
      try {
        const metadata = JSON.parse(text.slice(start, index + 1)) as Record<string, unknown>;
        const id = metadata.id ?? metadata.artifact_id;
        return typeof id === "string" && /^artifact-[a-f0-9]{32}$/u.test(id)
          && ["markdown", "chart", "grid"].includes(String(metadata.kind)) ? id : null;
      } catch { return null; }
    }
  }
  return null;
}
