/** Only user-authored request/correction text enters an export, never answer metadata or Memory bodies. */

const CREDENTIALS: Array<[RegExp, string]> = [
  [/-----BEGIN [^-]*PRIVATE KEY-----[\s\S]*?-----END [^-]*PRIVATE KEY-----/g, "[credential removed]"],
  [/\bBearer\s+\S+/gi, "[credential removed]"],
  [/\bAKIA[0-9A-Z]{16}\b/g, "[credential removed]"],
  [/\bxox[abprs]-[A-Za-z0-9-]{10,}/g, "[credential removed]"],
  [/\bAIza[0-9A-Za-z\-_]{20,}/g, "[credential removed]"],
  [/\bsk-[\w-]{8,}/g, "[credential removed]"],
  [/\bgh[pousr]_[A-Za-z0-9_]{16,}/g, "[credential removed]"],
  [/\beyJ[\w-]+\.[\w-]+\.[\w-]+/g, "[credential removed]"],
  [/\b[\w.-]*(?:key|secret|token|password)[\w.-]*\s*[:=]\s*(?:"[^"\n]*"|'[^'\n]*'|\S+)/gi, "[credential removed]"],
  [/(?<![A-Za-z0-9])[A-Fa-f0-9]{32,}(?![A-Za-z0-9])/g, "[credential removed]"],
  [/(?<![A-Za-z0-9+/])[A-Za-z0-9+/]{40,}={0,2}(?![A-Za-z0-9+/])/g, "[credential removed]"],
  [/(?:[a-z][a-z0-9+.-]*:\/\/|(?<![\w.])www\.)[^\s"'<>)\]]+/gi, "[link removed]"],
];

function pathEnd(text: string, start: number): number {
  const pathChar = /[^\s"'<>|]/;
  let index = start;
  while (index < text.length && pathChar.test(text[index] ?? "")) index += 1;
  while (index < text.length && (text[index] === " " || text[index] === "\t")) {
    let next = index + 1;
    while (next < text.length && pathChar.test(text[next] ?? "")) next += 1;
    const segment = text.slice(index + 1, next);
    if (!segment || !(/[\\/]/.test(segment) || /\.[A-Za-z0-9]{1,8}$/.test(segment))) break;
    index = next;
  }
  const span = text.slice(start, index);
  const kept = /(?:~\/|\.\.\/|\.\/|\/|\\)[^\s"'<>|]/.test(span)
    || /^(?:Users|home)\/\S/.test(span)
    || /^[A-Za-z]:[\\/]\S/.test(span);
  return kept ? index : start;
}

function scrubLocalPaths(text: string): string {
  const pathStart = /~\/|(?<![\w./])(?:\.\.\/|\.\/)+|(?<![\w:/])\/|(?<![\w])(?:Users|home)\/|(?<![A-Za-z0-9])[A-Za-z]:[\\/]|\\\\/g;
  let out = "";
  let cursor = 0;
  for (const match of text.matchAll(pathStart)) {
    const index = match.index ?? 0;
    if (index < cursor) continue;
    const end = pathEnd(text, index);
    if (end <= index) continue;
    out += text.slice(cursor, index) + "[local path removed]";
    cursor = end;
  }
  return out + text.slice(cursor);
}

export function portableFeedbackText(text: string): string {
  const stripped = CREDENTIALS.reduce((current, [pattern, replacement]) => current.replace(pattern, replacement), text);
  return scrubLocalPaths(stripped).trim();
}

/** Same threshold as `isKoreanUserTurn` in the agent suite contract: Hangul count ≥ 12 and greater than Latin. */
function hangulDominant(text: string): boolean {
  const hangul = (text.match(/[가-힣]/g) ?? []).length;
  const latin = (text.match(/[A-Za-z]/g) ?? []).length;
  return hangul >= 12 && hangul > latin;
}

function languageTag(text: string): "ko" | "ja" | "zh" | "en" {
  if (hangulDominant(text)) return "ko";
  if (/[\u3040-\u30ff]/u.test(text)) return "ja";
  if (/[\u4e00-\u9fff]/u.test(text)) return "zh";
  return "en";
}

export function buildFeedbackEvalCase(
  messageId: string,
  request: string,
  correction: string,
  labels = {
    title: "User-corrected answer",
    missingInput: "An original request and saved correction are required.",
  },
) {
  const user = portableFeedbackText(request);
  const corrected = portableFeedbackText(correction);
  if (!user || !corrected) throw new Error(labels.missingInput);
  const language = languageTag(user + corrected);
  return {
    id: `FB-${messageId.replace(/[^a-zA-Z0-9-]/g, "").slice(-16) || "answer"}`,
    title: labels.title,
    split: "regression",
    tags: ["user-feedback", language],
    connections: [],
    sessions: [{ id: "probe-1", role: "probe", memory: "off", turns: [{ user, expect: { rubric: [{
      id: "user-correction", criterion: `The answer must address the original request accurately and satisfy this user correction: ${corrected}`,
      metric: "task_success",
    }] } }] }],
  };
}
