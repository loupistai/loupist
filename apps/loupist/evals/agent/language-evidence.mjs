/** Language evidence for an answer’s own words, independent of appearance. */
import { eld } from 'eld/small';
import { fromMarkdown } from 'mdast-util-from-markdown';
import { answerLanguageAssertion, answerLanguageScript } from './eval-contract.mjs';
import { stripVisibleLabels } from './label-evidence.mjs';

// The English and Korean shares preserve the thresholds calibrated on R0/R1.
// The Japanese script combines Han and kana, as in the answer-feedback tag.
const MIN_SCRIPT_SHARE = { en: 0.7, ko: 0.35, ja: 0.35 };
const LETTERS = /\p{L}/gu;
const EAST_ASIAN = /[\p{Script_Extensions=Hani}\p{Script_Extensions=Hira}\p{Script_Extensions=Kana}]/u;
const DETECTOR_LANGUAGES = new Set(Object.values(eld.info().Languages));

const SKIPPED = new Set(['code', 'inlineCode', 'blockquote', 'html', 'image', 'definition']);
const BLOCKS = new Set(['root', 'paragraph', 'heading', 'list', 'listItem', 'table', 'tableRow', 'tableCell']);

/**
 * Prose the answer writes in its own words: link text without targets, and no code, quotations,
 * parentheses, tickers, acronyms or unit letters on a number, which read the same in either language.
 */
export function ownWords(answer) {
  const plain = (node) => SKIPPED.has(node.type) ? ''
    : node.type === 'text' ? node.value : (node.children ?? []).map(plain).join(BLOCKS.has(node.type) ? '\n' : '');
  let text = plain(fromMarkdown(String(answer ?? '')))
    .replace(/\bhttps?:\/\/\S+/gu, ' ')
    // A quotation, single quotes included; an apostrophe between letters ("company’s") does not close one.
    .replace(/"[^"\n]*"|“[^”\n]*”|‘(?:[^’\n]|(?<=[A-Za-z])’(?=[A-Za-z]))*’|(?<![A-Za-z])'(?:[^'\n]|(?<=[A-Za-z])'(?=[A-Za-z]))*'(?![A-Za-z])|「[^」\n]*」|『[^』\n]*』|«[^»\n]*»/gu, ' ')
    .replace(/\b\d[\d.,]*[A-Za-z]{1,3}\b/gu, ' ')
    .replace(/\b[A-Z][A-Z0-9&.-]*[A-Z0-9]\b/gu, ' ');
  for (let previous = ''; previous !== text;) {
    previous = text;
    text = text.replace(/\([^()\n]*\)|（[^（）\n]*）/gu, ' ');
  }
  return text;
}

function proseBlocks(answer) {
  const blocks = [];
  const visit = (node) => {
    if (node.type === 'paragraph' || node.type === 'heading') blocks.push(node);
    else if (!SKIPPED.has(node.type)) (node.children ?? []).forEach(visit);
  };
  const text = String(answer ?? '');
  visit(fromMarkdown(text));
  return blocks.map(({ position }) => text.slice(position.start.offset, position.end.offset));
}

export function answerLanguageEvidence(assertion, answer, labels = []) {
  const tag = answerLanguageAssertion(assertion);
  const { language, script } = new Intl.Locale(tag).maximize();
  const profile = { language, script: answerLanguageScript(script), minimum: MIN_SCRIPT_SHARE[language] ?? 0.5 };
  if (assertion.per_paragraph) {
    const prose = proseBlocks(answer).map(block => stripVisibleLabels(ownWords(block), labels))
      .filter(words => /\p{L}/u.test(words));
    if (!prose.length) return { pass: false, detail: 'no explanatory prose' };
    const results = prose.map(words => languageEvidence(profile, words));
    const failing = results.findIndex(result => result.pass === false);
    if (failing !== -1) return { pass: false, detail: `prose block ${failing + 1}: ${results[failing].detail}` };
    return results.find(result => result.pass === null)
      ?? { pass: true, detail: `${prose.length} prose blocks in ${tag}` };
  }

  const words = ownWords(answer);
  // A standalone capitalized abbreviation such as “OK” is the whole reply.
  if (language === 'en' && !/\p{L}/u.test(words) && /^\s*[A-Z]+[.!?]?\s*$/u.test(String(answer ?? ''))) {
    return { pass: true, detail: 'standalone English abbreviation' };
  }
  return languageEvidence(profile, words);
}

function languageEvidence({ language, script, minimum }, words) {
  const normalized = words.normalize('NFC');
  const letters = normalized.match(LETTERS) ?? [];
  if (!letters.length) return { pass: false, detail: 'no letters in the answer’s own words' };
  const share = letters.filter(letter => script.test(letter)).length / letters.length;
  const coverage = `${Math.round(share * 100)}% in the script for ${language}`;
  if (share <= minimum) return { pass: false, detail: `${coverage}, need above ${minimum * 100}%` };
  if (language === 'ko') return { pass: true, detail: coverage }; // Distinct Hangul script.
  if (!DETECTOR_LANGUAGES.has(language)) return { pass: null, detail: `no language model for ${language}` };

  // Drop names in other scripts before detection. Keep both Han and kana for
  // Japanese and Chinese so their shared Han letters do not hide the distinction.
  const detectorScript = language === 'ja' || language === 'zh' ? EAST_ASIAN : script;
  const prose = normalized.replace(LETTERS, letter => detectorScript.test(letter) ? letter : ' ').trim();
  // “No” is a complete answer in English and in several other languages.
  const detected = language === 'en' && /^no[.!?]?$/iu.test(prose) ? 'en' : eld.detect(prose).language;
  return { pass: detected === language, detail: `${coverage}, detected ${detected || 'no language'}` };
}
