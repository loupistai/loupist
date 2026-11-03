const ascii = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const accented = "áƀçďëƒğħïĵķľɱñôþɋřšŧüṽŵẋýžÁɃÇĎËƑĞĦÏĴĶĽṀÑÔÞɊŘŠŦÜṼŴẊÝŽ";
const accents = Object.fromEntries([...ascii].map((letter, index) => [letter, [...accented][index]]));

function pseudoLocalize(text: string): string {
  let visibleLength = 0;
  const transformed = text.split(/(\{\{[\s\S]*?\}\}|<\/?\d+\s*\/?>)/g).map((part) => {
    if (/^(\{\{|<\/?\d)/.test(part)) return part;
    visibleLength += part.length;
    return part.replace(/[a-z]/gi, (letter) => accents[letter]);
  }).join("");
  return `⟦${transformed}${"·~".repeat(Math.round(visibleLength * 0.35 / 2))}⟧`;
}

export function pseudoResources<T>(value: T): T {
  if (typeof value === "string") return pseudoLocalize(value) as T;
  if (Array.isArray(value)) return value.map(pseudoResources) as T;
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, pseudoResources(entry)])) as T;
  }
  return value;
}
