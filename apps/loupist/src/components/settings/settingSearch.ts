import i18n, { resources } from "@/i18n";
import { primaryLanguage } from "@/i18n/localized";
import type { SettingsSection } from "../../app/navigation";

export type SettingResult = { section: SettingsSection; label: string; description: string };
export const normalizeSetting = (text: string) => text.normalize("NFKC").toLocaleLowerCase().trim();

function labelText(node: Element): string {
  const walker = node.ownerDocument.createTreeWalker(node, NodeFilter.SHOW_TEXT);
  const words: string[] = [];
  while (walker.nextNode()) {
    if (!walker.currentNode.parentElement?.closest('textarea,select,svg,[hidden]:not([data-setting-disclosure]),[aria-hidden="true"],.sr-only')) {
      words.push(walker.currentNode.textContent ?? "");
    }
  }
  return words.join("").trim();
}

/** Read setting names and help text from the controls themselves, including
 * advanced controls inside a disclosure. Never index the contents of an input. */
export function settingControls(page: ParentNode, section: SettingsSection) {
  const results = new Map<string, { result: SettingResult; element: HTMLElement }>();
  const elements = page.querySelectorAll<HTMLElement>('label, h2, h3, h4, strong, summary, button, [role="group"][aria-label], [role="switch"], select, input[aria-label], tbody th, tbody td:first-child');
  for (const element of elements) {
    if (section === "usage" && !element.matches(".usage-period, .usage-period button, .usage-group > h2, .usage-plan button")) continue;
    if (element.closest('[hidden]:not([data-setting-disclosure]),[aria-hidden="true"],[role="dialog"],.sr-only') || element.matches("input[type=password]")) continue;
    const heading = element.querySelector("strong, h3, h4, .skill-title");
    const label = (element.getAttribute("aria-label") || (heading && labelText(heading)) || (element.tagName === "SUMMARY" ? element.querySelector("span")?.firstChild?.textContent : element.matches("input,select") ? "" : labelText(element)))?.trim();
    if (!label || results.has(label)) continue;
    const row = element.closest(".settings-row,.agent-config-field,.settings-model-row,.skill-row,.marketplace-card,.provider-card-heading,tr");
    const next = (element.closest("h2,h3,h4,label") ?? element).nextElementSibling;
    const help = row?.querySelectorAll("p,small,kbd,.skill-description,.provider-card-meta") ?? element.querySelectorAll("p,small");
    const descriptions = Array.from(help, node => labelText(node)).filter(Boolean);
    if (next?.matches("p,small")) descriptions.push(labelText(next));
    const control = element instanceof HTMLLabelElement ? element.control : element;
    for (const id of control?.getAttribute("aria-describedby")?.split(/\s+/u) ?? []) {
      const node = page.querySelector(`#${CSS.escape(id)}`);
      if (node?.textContent) descriptions.push(node.textContent.trim());
    }
    results.set(label, { result: { section, label, description: [...new Set(descriptions)].join(" ") }, element });
  }
  return results;
}

/** Build the translation lookup from shipped resources, so a search in the other
 * language uses the same names that its Settings pages display. */
export function settingTranslations(language: string) {
  const aliases = new Map<string, string[]>();
  const templates: { prefix: string; suffix: string; placeholder: string; other: string }[] = [];
  const collect = (current: unknown, other: unknown) => {
    if (typeof current === "string" && typeof other === "string") {
      const key = normalizeSetting(current);
      const translated = normalizeSetting(other);
      const slot = /^(.*?)(\{\{[^{}]+\}\})([^{}]*)$/u.exec(key);
      if (slot && (slot[1] || slot[3]) && !slot[1].includes("{{") && translated.includes(slot[2])) {
        templates.push({ prefix: slot[1], suffix: slot[3], placeholder: slot[2], other: translated });
      } else {
        aliases.set(key, [...(aliases.get(key) ?? []), translated]);
      }
    } else if (current && other && typeof current === "object" && typeof other === "object") {
      for (const [key, value] of Object.entries(current)) collect(value, (other as Record<string, unknown>)[key]);
    }
  };
  const current = i18n.getDataByLanguage(language);
  // Every other bundled language, so a search in any of them finds the page shown in this one.
  const others = Object.keys(resources).filter(other => primaryLanguage(other) !== primaryLanguage(language)).map(other => i18n.getDataByLanguage(other));
  for (const other of others) for (const namespace of ["app", "settings", "connections", "onboarding", "common", "sharing"]) collect(current?.[namespace], other?.[namespace]);
  return (key: string): string[] => {
    const existing = aliases.get(key);
    if (existing) return existing;
    const found = templates.flatMap(({ prefix, suffix, placeholder, other }) => {
      if (!key.startsWith(prefix) || !key.endsWith(suffix) || key.length <= prefix.length + suffix.length) return [];
      return other.replace(placeholder, () => key.slice(prefix.length, key.length - suffix.length));
    });
    aliases.set(key, found);
    return found;
  };
}

export function matchesSetting(label: string, description: string, query: string, aliases: (key: string) => string[]) {
  const key = normalizeSetting(label);
  return [key, normalizeSetting(description), ...aliases(key)].some(text => text.includes(query));
}

export function focusSetting(element: HTMLElement) {
  const disclosure = element.closest<HTMLElement>("[data-setting-disclosure][hidden]");
  if (disclosure) {
    disclosure.closest(".settings-page")?.querySelector<HTMLButtonElement>(`[aria-controls="${CSS.escape(disclosure.id)}"]`)?.click();
    return false;
  }
  for (let parent = element.parentElement; parent; parent = parent.parentElement) {
    if (parent instanceof HTMLDetailsElement) parent.open = true;
  }
  const label = element.closest("label") ?? element.querySelector("label");
  const title = labelText(element);
  const group = element.matches(".usage-period") ? element
    : title && element.matches("h2,h3,h4") ? element.closest(".settings-page")?.querySelector<HTMLElement>(`[role="group"][aria-label="${CSS.escape(title)}"]`) : null;
  const choice = group?.querySelector<HTMLElement>('[aria-pressed="true"],[aria-checked="true"]') ?? group?.querySelector<HTMLElement>("button");
  const control = label?.control ?? choice ?? (element.matches("button,input,select,summary,[tabindex]") ? element : undefined)
    ?? element.closest("button") ?? element.closest(".settings-row,.agent-config-field,tr")?.querySelector<HTMLElement>("button:not(:disabled), select, input");
  const target = control instanceof HTMLElement && !control.matches(":disabled") ? control : element;
  if (target.tabIndex < 0) target.tabIndex = -1;
  target.setAttribute("data-setting-target", "");
  target.addEventListener("blur", () => target.removeAttribute("data-setting-target"), { once: true });
  target.focus({ preventScroll: true });
  target.scrollIntoView?.({ block: "nearest" });
  return true;
}
