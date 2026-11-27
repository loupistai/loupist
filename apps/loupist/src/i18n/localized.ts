/**
 * Language-neutral lookups for text that arrives keyed by language (host labels,
 * bundled data): code asks for the interface language's entry and never names a
 * language itself.
 */

/** The primary language subtag, lowercased ("pt-BR" → "pt", "en-XA" → "en"). */
export const primaryLanguage = (language: string) => language.toLowerCase().split(/[-_]/u)[0] ?? "";

/**
 * The value for the interface language, else the English one. `usable` rejects an
 * entry that cannot stand in (an empty label, a list of the wrong length).
 */
export function localizedValue<T>(values: Readonly<Record<string, T | null | undefined>> | null | undefined, language: string, usable: (value: T) => boolean = Boolean): T | undefined {
  if (!values) return undefined;
  for (const key of [primaryLanguage(language), "en"]) {
    const value = values[key];
    if (value != null && usable(value)) return value;
  }
  return undefined;
}

/** A wide character (Hangul, kana, CJK, full-width forms) takes this much of a Latin character's room. */
const WIDE_SHARE = 64 / 44;
const WIDE = /[ᄀ-ᅟ⺀-〾ぁ-㏿㐀-䶿一-鿿ꥠ-꥿가-힣豈-﫿︰-﹏＀-｠￠-￦]/u;

/** How much room a line takes, in Latin characters, so one budget fits every script. */
export function displayWidth(text: string) {
  let width = 0;
  for (const char of text) width += WIDE.test(char) ? WIDE_SHARE : 1;
  return width;
}
