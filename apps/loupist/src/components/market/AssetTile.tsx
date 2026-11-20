type Props = {
  /** The asset's name in the interface language ("Samsung Electronics", "삼성전자"). */
  name: string;
  /** Used when the name is empty. */
  symbol?: string;
  size?: "sm" | "md" | "lg";
};

// A company form is not part of the name it abbreviates: "Apple Inc." is "AP", not "AI".
const LEGAL_SUFFIX = /^(?:inc|corp|corporation|co|company|ltd|limited|plc|llc|ag|sa|se|nv|holdings?|group)$/iu;
// Hangul, kana and CJK ideographs: one character names the asset ("삼", "エ", "腾").
const SINGLE_GLYPH = /^[\p{Script=Hangul}\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]/u;

/**
 * The tile's letters, taken from the name in the current language: the first
 * character of a CJK name; else the initials of two words ("SE"), or the first two
 * letters of one. An all-capital first word gives its own first two ("SK hynix" is "SK",
 * "NVIDIA Corp" is "NV").
 */
function monogram(name: string, symbol = "") {
  const source = name.trim() || symbol.trim();
  if (!source) return "";
  if (SINGLE_GLYPH.test(source)) return Array.from(source)[0];
  const words = source.split(/[\s\-–—_.,&/()]+/u).filter((word, index) => word && (index === 0 || !LEGAL_SUFFIX.test(word)));
  const first = words[0] ?? source;
  const letters = (text: string) => Array.from(text).filter((character) => /[\p{L}\p{N}]/u.test(character));
  const firstLetters = letters(first);
  const capitalWord = firstLetters.length >= 2 && first === first.toUpperCase() && /\p{Lu}/u.test(first);
  if (words.length > 1 && !capitalWord) {
    const second = letters(words[1])[0];
    if (firstLetters[0] && second) return `${firstLetters[0]}${second}`.toLocaleUpperCase();
  }
  return firstLetters.slice(0, 2).join("").toLocaleUpperCase() || Array.from(source).slice(0, 2).join("");
}

/** The square monogram tile beside an asset's name. Identity, not an icon: the name stays readable beside it. */
export function AssetTile({ name, symbol, size = "md" }: Props) {
  return <span className="market-tile" data-size={size} aria-hidden="true">{monogram(name, symbol)}</span>;
}
