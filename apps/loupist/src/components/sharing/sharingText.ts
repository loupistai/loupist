import i18n from "@/i18n";
import type { PackLicense } from "../../contracts/sharing";

/** `loupist-<slug>`: the default repository name for a Loupist. */
export function repoSlug(name: string) {
  const slug = name.normalize("NFKD").replace(/[̀-ͯ]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 60);
  return `loupist-${slug || "pack"}`;
}

type PublicLicense = Exclude<PackLicense, "LicenseRef-Loupist-Personal">;

/** Creative Commons names are proper names and stay untranslated; the reserved Personal license is named in the UI language. */
const CC_NAMES: Record<PublicLicense, string> = {
  "CC-BY-NC-SA-4.0": "CC BY-NC-SA 4.0", "CC-BY-NC-4.0": "CC BY-NC 4.0", "CC-BY-4.0": "CC BY 4.0", "CC-BY-SA-4.0": "CC BY-SA 4.0", "CC0-1.0": "CC0 1.0",
};
export function licenseName(license: PackLicense) {
  return license === "LicenseRef-Loupist-Personal" ? i18n.t("sharing:licensePersonal") : CC_NAMES[license];
}

/** What anyone may do with content under a public license, in one plain line (`sharing:licenseSummary.*`). */
const SUMMARY_KEYS = { "CC-BY-NC-SA-4.0": "byNcSa", "CC-BY-NC-4.0": "byNc", "CC-BY-4.0": "by", "CC-BY-SA-4.0": "bySa", "CC0-1.0": "zero" } as const satisfies Record<PublicLicense, string>;
export function licenseSummary(license: PackLicense) {
  return license === "LicenseRef-Loupist-Personal" ? null : i18n.t(`sharing:licenseSummary.${SUMMARY_KEYS[license]}`);
}

/** A link shown as text with its domain: Markdown links never become live links in shared text. */
export function plainInline(text: string) {
  return text
    .replace(/<[^>]*>/g, "")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]+)\]\(([^)\s]+)[^)]*\)/g, (_match, label: string, href: string) => {
      try { return `${label} (${new URL(href).hostname})`; } catch { return label; }
    })
    .replace(/(\*\*|__)(.+?)\1/g, "$2");
}

/** A record title quoted in a sentence: whitespace collapsed, cut at a word boundary with an ellipsis past `limit` characters. */
export function shortTitle(text: string, limit = 48) {
  const clean = text.replace(/\s+/gu, " ").trim();
  const chars = [...clean];
  if (chars.length <= limit) return clean;
  const cut = chars.slice(0, limit).join("");
  const space = cut.lastIndexOf(" ");
  return `${(space > limit / 2 ? cut.slice(0, space) : cut).replace(/[\s,.;:–—-]+$/u, "")}…`;
}
