import i18n from "@/i18n";
import english from "@/i18n/locales/en/connections.json";

// The host supplies bundled English metadata. Resolve that copy through typed
// catalog keys; provider names and unknown third-party metadata stay intact.
const copyKeys = new Map(Object.entries(english.catalog).map(([key, value]) => [value, key as keyof typeof english.catalog]));

export function connectionText(value: string): string {
  const key = copyKeys.get(value);
  return key ? i18n.t(`connections:catalog.${key}`) : value;
}
