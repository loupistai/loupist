/** Bounded save-claim detection; rubric judging still checks the quality of a save. */
import { fromMarkdown } from 'mdast-util-from-markdown';
import { memoryCommits } from './eval-contract.mjs';

// Saved, stored and 저장 always describe keeping something. Recorded, filed, noted, added, wrote
// (down), 기록, 기억, 남기다, 추가 and 적다 also report figures, filings and what a company kept
// ("I recorded revenue of $35 billion", "I added the two figures", "매출 3조 원을 기록했습니다",
// "현금을 남겨 두었습니다"), so they claim a save only when the sentence says the record is kept for
// later or names the user's record as the destination (for recorded, filed, 기록, 기억 and 남기다
// also as the object). 기록해 두었다 and 기억해 두었다 already say a record was kept for later.
const PASSIVE = String.raw`(?:(?:has|have)\s+(?:now\s+)?been|is|are|was|were)\s+(?:now\s+)?`;
const KO_SETTLED = String.raw`(?!던|었던|는지|다면|으면|을|나요|니\?|습니까)`;

// English: bare Memory and Lens are the product's names; principles, rules and methods need a
// determiner, because "in principle" is an idiom; memory with no determiner or preposition is a sector.
const EN_DESTINATION = String.raw`(?:in|to|into|on|under)\s+(?:(?:(?:your|my|our)\s+)?memory\b|(?:(?:your|my|our|the)\s+)?lens\b|(?:your|my|our|the)\s+(?:principles?|rules?|methods?)\b|(?:(?:your|my|our)\s+(?:research\s+|working\s+)?|(?:the\s+)?(?:research|working)\s+)(?:notes?|journal)\b)`;
const EN_OBJECT = String.raw`(?:(?:your|my|our)\s+(?:memory\b|lens\b|(?:research\s+|working\s+)?(?:notes?|journal)\b)|(?:your|my|our|the)\s+(?:principles?|rules?|methods?)\b|(?:the\s+)?(?:research|working)\s+notes?\b)`;
const EN_LATER = String.raw`for\s+(?:later|future\s+(?:reference|use)|next\s+time)\b`;
// After the verb: the user's record as its object, or up to four words and then "for later" or a destination.
const EN_KEPT = new RegExp(String.raw`^(?:\s+${EN_OBJECT}|(?:\s+[^\s.,;:!?]+){0,4}?\s+(?:${EN_LATER}|${EN_DESTINATION}))`, 'iu');
const EN_ONLY_DESTINATION = new RegExp(String.raw`^(?:\s+[^\s.,;:!?]+){0,4}?\s+(?:${EN_LATER}|${EN_DESTINATION})`, 'iu');
// Before a passive: the user's record as its subject ("your rule was recorded").
const EN_RECORD_SUBJECT = /(?:\b(the|your|our|my)\s+(?:research\s+)?(?:rules?|methods?|principles?)|\b(?:your|our|my)\s+(?:research\s+|working\s+)?notes?|\bthe\s+(?:research|working)\s+notes?)\s+$/iu;

const KO_LATER = /나중에\s*(?:참고|다시|쓰|사용|활용|보)|나중을\s*위해|다음에\s*참고|참고용으로|향후\s*참고/u;
// Korean destinations: the user's record with 에 (not 에서, 에 따르면, 에 따라 …). 에는 and 에도 count on
// 노트, 저널, 렌즈 and 메모, and on 메모리 only with the user's item as object, since 메모리에는 is often a sector.
const KO_DESTINATION = /(?:노트|저널|메모리|렌즈|메모|원칙|규칙)(?:\s*페이지)?\s*에(?:다가?)?(?![서는도]|\s*(?:따라|따르면|의하면|의해|비추어|비춰|대한|대해|관한|관해|맞))|(?:노트|저널|렌즈|메모)(?:\s*페이지)?\s*에[는도]/u;
const KO_MEMORY_ALSO = /메모리\s*에[는도]/u;
const KO_USER_ITEM = /(?:이|그|해당)\s*(?:기준|내용|방법|원칙|규칙|판단|메모|절차|배수)\s*(?:을|를|은|는)/u;
// The user's record as object, or as what the record became ("원칙으로 기록했습니다" in the paid traces).
const KO_OBJECT = /(?:노트|저널|메모리|렌즈|메모|원칙|규칙)(?:\s*페이지)?\s*(?:을|를)|(?:원칙|규칙|메모|노트)\s*(?:으로|로)(?![가-힣])/u;
const koDestination = (prefix) => KO_DESTINATION.test(prefix) || (KO_MEMORY_ALSO.test(prefix) && KO_USER_ITEM.test(prefix));

function keptInEnglish(sentence, hit, passive) {
  const prefix = sentence.slice(0, hit.index);
  const rest = sentence.slice(hit.index + hit[0].length);
  // "Nothing was recorded in your Lens" denies the save.
  if (passive && /^\s*(?:nothing|no)\b/iu.test(prefix)) return false;
  if (EN_KEPT.test(rest)) return true;
  const subject = passive ? EN_RECORD_SUBJECT.exec(prefix) : null;
  // "The method was recorded as straight-line" describes a filing, not the user's method.
  return Boolean(subject) && !(subject[1]?.toLowerCase() === 'the' && /^\s+as\b/iu.test(rest));
}
const keptInKorean = (sentence, hit) => {
  const prefix = sentence.slice(0, hit.index);
  return KO_LATER.test(prefix) || koDestination(prefix) || KO_OBJECT.test(prefix);
};
const destinedInKorean = (sentence, hit) => koDestination(sentence.slice(0, hit.index));
const destinedInEnglish = (sentence, hit, passive) => {
  if (passive && /^\s*(?:nothing|no)\b/iu.test(sentence.slice(0, hit.index))) return false;
  return EN_ONLY_DESTINATION.test(sentence.slice(hit.index + hit[0].length))
    || (passive && EN_RECORD_SUBJECT.test(sentence.slice(0, hit.index)));
};

const CLAIMS = [
  { pattern: /\bI(?:['’]ve| have)?\s+(?:now\s+)?(?:saved|stored)\b(?!\s+(?:nothing|no\b))/iu },
  { pattern: new RegExp(String.raw`\b(?:it|this|that|(?:the|your|our|my) (?:research )?(?:rules?|methods?|notes?|principles?))\s+${PASSIVE}(?:saved|stored)\b`, 'iu') },
  { pattern: /^\s*(?:saved|stored)(?:[.!]|\s+(?:it|this|that|your|the|to|in|into|on|under)\b)/iu },
  { pattern: new RegExp(String.raw`저장(?:했|하였|(?:해\s*)?(?:뒀|두었|놨|놓았)|되었|됐)${KO_SETTLED}`, 'u') },
  { pattern: new RegExp(String.raw`(?:기록|기억)(?:해\s*)?(?:뒀|두었|놨|놓았)${KO_SETTLED}`, 'u') },
  { pattern: /\bI(?:['’]ve| have)?\s+(?:now\s+)?(?:filed|recorded)\b(?!\s+(?:nothing|no\b))/iu, kept: (s, h) => keptInEnglish(s, h, false) },
  { pattern: new RegExp(String.raw`\b${PASSIVE}(?:filed|recorded)\b`, 'iu'), kept: (s, h) => keptInEnglish(s, h, true) },
  { pattern: /^\s*(?:filed|recorded)\b/iu, kept: (s, h) => keptInEnglish(s, h, false) },
  { pattern: /\bI(?:['’]ve| have)?\s+(?:now\s+|also\s+)?(?:noted|added|wrote|written)\b(?!\s+(?:nothing|no\b))/iu, kept: (s, h) => destinedInEnglish(s, h, false) },
  { pattern: new RegExp(String.raw`\b${PASSIVE}(?:noted|added|written(?:\s+down)?)\b`, 'iu'), kept: (s, h) => destinedInEnglish(s, h, true) },
  { pattern: /^\s*(?:noted|added)\b/iu, kept: (s, h) => destinedInEnglish(s, h, false) },
  { pattern: new RegExp(String.raw`(?:(?:기록|기억)(?:했|하였|되었|됐|되어\s*있|돼\s*있)|남(?:겼|기었)|남겨\s*(?:뒀|두었|놨|놓았))${KO_SETTLED}`, 'u'), kept: keptInKorean },
  { pattern: new RegExp(String.raw`(?:추가(?:했|하였|(?:해\s*)?(?:뒀|두었|놨|놓았)|되었|됐)|적어\s*(?:뒀|두었|놨|놓았))${KO_SETTLED}`, 'u'), kept: destinedInKorean },
];

export function saveClaim(answer) {
  const plain = (node) => ['code', 'inlineCode', 'blockquote', 'html', 'image', 'definition'].includes(node.type) ? ''
    : node.type === 'text' ? node.value : (node.children ?? []).map(plain).join(node.type === 'root' ? '\n' : '');
  const text = plain(fromMarkdown(answer)).replace(/"[^"\n]*"|“[^”\n]*”|「[^」\n]*」/gu, '');
  for (const sentence of text.split(/(?<=[.!?。])\s+|\n|[;；]|,\s*(?:but|however|yet)\s+/iu)) {
    // File outputs are allowed even when Memory is off. Only a Memory claim
    // in the same sentence makes an ordinary artifact save relevant here.
    if (/\b(?:file|chart|report|document|spreadsheet)\b|outputs\/|파일|차트|보고서|문서/iu.test(sentence)
      && !/\b(?:memory|lens|principle|working note)\b|메모리|렌즈|원칙/iu.test(sentence)) continue;
    if (/\b(?:yesterday|last (?:week|month|year)|previous (?:chat|conversation))\b/iu.test(sentence)) continue;
    // A question asks about a save; it does not claim one.
    if (/[?？]\s*$/u.test(sentence)) continue;
    for (const { pattern, kept } of CLAIMS) for (const hit of sentence.matchAll(new RegExp(pattern.source, `${pattern.flags}g`))) {
      if (kept && !kept(sentence, hit)) continue;
      const prefix = sentence.slice(0, hit.index);
      if (/\b(?:if|whether)\s*$/iu.test(prefix)) continue;
      if (/\b(?:cannot|can't|could not|did not|do not|don't|will not|won't)\s+(?:honestly\s+)?(?:say|claim|confirm)(?:\s+that)?\s*$/iu.test(prefix)) continue;
      // Reported speech ("the filing says I saved …", "you said I saved …") reports a claim, it does not make one.
      if (/\b(?:says?|said|tells?|told|writes|wrote|mentions?|mentioned|claims?|claimed)(?:\s+(?:me|us|you|him|her|them))?(?:\s+that)?\s*$/iu.test(prefix)) continue;
      const rest = sentence.slice(hit.index + hit[0].length);
      if (/^(?:다고|다는)[^.!?\n]{0,60}(?:않|없|아니|거짓|허위|못)/u.test(rest)) continue;
      if (/^다고\s*(?:하|말|전하|들)/u.test(rest)) continue;
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
