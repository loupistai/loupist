/**
 * Korean particles that depend on the sound before them. Catalog strings write
 * both forms after a value ("{{name}}이(가)"); one i18next post-processor picks
 * the form by the final sound of the text in front of it, so every string that
 * follows this convention reads naturally without per-call code.
 */

/** The final sound of a word as Korean readers say it; undefined when unknown. */
export type FinalSound = "vowel" | "rieul" | "consonant";

/** Closing quotes and brackets do not change the particle: ‘깨짐’으로, “Hanul”이. */
const TRAILING = /[\s’”'"»」』〉》)\]}]+$/u;
/** The Korean reading of each digit ends in: 영 일 이 삼 사 오 육 칠 팔 구. */
const DIGIT: Record<string, FinalSound> = { 0: "consonant", 1: "rieul", 2: "vowel", 3: "consonant", 4: "vowel", 5: "vowel", 6: "consonant", 7: "rieul", 8: "rieul", 9: "vowel" };
/** Letter names in an acronym (KRX, IBM): 엘, 엠, 엔 and 알 end in a consonant. */
const LETTER: Record<string, FinalSound> = { L: "rieul", M: "consonant", N: "consonant", R: "rieul" };

export function finalSound(text: string): FinalSound | undefined {
  const trimmed = text.replace(TRAILING, "");
  const last = trimmed.slice(-1);
  if (!last) return undefined;
  const code = last.charCodeAt(0);
  if (code >= 0xac00 && code <= 0xd7a3) {
    const final = (code - 0xac00) % 28;
    return final === 0 ? "vowel" : final === 8 ? "rieul" : "consonant";
  }
  if (/\d/u.test(last)) {
    // 10 십, 100 백, 1,000 천, 10,000 만: a round number ends in a consonant.
    if (last === "0" && /\d0$/u.test(trimmed.replace(/[,.]/gu, ""))) return "consonant";
    return DIGIT[last];
  }
  if (last === "%") return "vowel"; // 퍼센트
  const word = /[A-Za-z]+$/u.exec(trimmed)?.[0];
  if (!word) return undefined;
  // A short or vowelless acronym reads letter by letter (KRX, SEC, TSMC); a longer one with
  // vowels reads as a word (EDGAR 에드거, KOSPI 코스피, AAPL 애플).
  if (word.length > 1 && word === word.toUpperCase() && (word.length < 4 || !/[AEIOU]/u.test(word))) return LETTER[word.slice(-1)] ?? "vowel";
  // Loanword sounds: Hanul 하눌, Apple 애플 (ㄹ); Precision 프리시전, Hong Kong 홍콩, Black 블랙;
  // most other endings take a vowel (Components 컴포넌츠, Compounder 컴파운더, Robert 로버트).
  if (/(?:l|le)$/iu.test(word)) return "rieul";
  if (/(?:m|n|ng|ck)$/iu.test(word)) return "consonant";
  return "vowel";
}

/** [written pair, form after a consonant, form after a vowel]. (으)로 takes 로 after ㄹ. */
const PAIRS: [string, string, string][] = [
  ["이(가)", "이", "가"], ["가(이)", "이", "가"],
  ["을(를)", "을", "를"], ["를(을)", "을", "를"],
  ["은(는)", "은", "는"], ["는(은)", "은", "는"],
  ["과(와)", "과", "와"], ["와(과)", "과", "와"],
  ["아(야)", "아", "야"], ["야(아)", "아", "야"],
  ["으로(로)", "으로", "로"], ["(으)로", "으로", "로"],
];
const escape = (text: string) => text.replace(/[()]/gu, "\\$&");
const PATTERN = new RegExp(PAIRS.map(([pair]) => escape(pair)).join("|"), "gu");
const FORMS = new Map(PAIRS.map(([pair, consonant, vowel]) => [pair, { consonant, vowel }]));

/** A closing bracket or quote right before a particle: lines may break after these, so the particle is joined. */
const CLOSING = /[’”'"»」』〉》)\]}]$/u;
/** U+2060 WORD JOINER: no line break between a closing bracket and its particle ("(2026.09)를"). */
const JOIN = "\u2060";

/** Resolves every particle pair in `text` by the sound before it; unknown sounds keep the pair. */
export function resolveKoreanParticles(text: string) {
  if (!text.includes("(")) return text;
  return text.replace(PATTERN, (pair, offset: number) => {
    const forms = FORMS.get(pair)!;
    const before = text.slice(0, offset);
    const sound = finalSound(before);
    if (!sound) return pair;
    // ㄹ before (으)로 takes 로: 서울로, 1로.
    const particle = sound === "vowel" ? forms.vowel : sound === "rieul" && forms.consonant === "으로" ? forms.vowel : forms.consonant;
    return CLOSING.test(before) ? `${JOIN}${particle}` : particle;
  });
}

/** The i18next post-processor that applies `resolveKoreanParticles` to every translation. */
export const koreanParticles = {
  type: "postProcessor" as const,
  name: "koreanParticles",
  process: (value: string) => resolveKoreanParticles(value),
};
