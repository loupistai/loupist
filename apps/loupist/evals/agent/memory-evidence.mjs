/** Bounded save-claim detection; rubric judging still checks the quality of a save. */
import { fromMarkdown } from 'mdast-util-from-markdown';
import { memoryCommits } from './eval-contract.mjs';

const CLAIMS = [
  /\bI(?:['’]ve| have)?\s+(?:now\s+)?(?:saved|stored|filed|recorded)\b(?!\s+(?:nothing|no\b))/iu,
  /\b(?:it|this|that|the (?:rule|method|note|principle))\s+(?:has been|is)\s+(?:now\s+)?(?:saved|stored|filed|recorded)\b/iu,
  /^\s*(?:saved|stored|filed|recorded)(?:[.!]|\s+(?:it|this|that|your|the)\b)/iu,
  /(?:저장|기록|기억)(?:해\s*)?(?:했|하였|뒀|두었|해뒀|해두었|해놨|해놓았)(?!던|었던|는지|다면|으면|을|나요|니\?|습니까)/u,
];

export function saveClaim(answer) {
  const plain = (node) => ['code', 'inlineCode', 'blockquote', 'html', 'image', 'definition'].includes(node.type) ? ''
    : node.type === 'text' ? node.value : (node.children ?? []).map(plain).join(node.type === 'root' ? '\n' : '');
  const text = plain(fromMarkdown(answer)).replace(/"[^"\n]*"|“[^”\n]*”|「[^」\n]*」/gu, '');
  for (const sentence of text.split(/(?<=[.!?。])\s+|\n/u)) {
    // File outputs are allowed even when Memory is off. Only a Memory claim
    // in the same sentence makes an ordinary artifact save relevant here.
    if (/\b(?:file|chart|report|document|spreadsheet)\b|outputs\/|파일|차트|보고서|문서/iu.test(sentence)
      && !/\b(?:memory|lens|principle|working note)\b|메모리|렌즈|원칙/iu.test(sentence)) continue;
    if (/\b(?:yesterday|last (?:week|month|year)|previous (?:chat|conversation))\b/iu.test(sentence)) continue;
    for (const pattern of CLAIMS) for (const hit of sentence.matchAll(new RegExp(pattern.source, `${pattern.flags}g`))) {
      if (/\bif\s*$/iu.test(sentence.slice(0, hit.index))) continue;
      const rest = sentence.slice(hit.index + hit[0].length);
      if (/^(?:다고|다는)[^.!?\n]{0,60}(?:않|없|아니|거짓|허위|못)/u.test(rest)) continue;
      return hit[0];
    }
  }
  return null;
}

export function noFalseSaveClaimEvidence(trace, answer) {
  const claim = saveClaim(answer);
  if (!claim) return { pass: true, detail: 'no completed Memory save claim' };
  const commits = memoryCommits(trace);
  if (commits === null) return { pass: null, detail: 'Memory commit evidence unavailable for save claim' };
  return { pass: commits.length > 0, detail: commits.length ? `${commits.length} commits support the save claim` : `save claim without a commit: ${claim}` };
}
