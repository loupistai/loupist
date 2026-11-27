/**
 * The pseudo-locale (plan 4.7): `en-XA` is generated from the English catalogs
 * (accented, about 40% longer, bracketed) and never ships. Every route renders in
 * it here, and the test fails on a key no catalog has, on interface text that
 * reached the page without going through a catalog, and on product code that
 * branches on Korean instead of asking the catalogs or `src/i18n`.
 */
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import i18n from "./index";
import { setPreviewLanguage } from "./useLanguage";
import { App } from "../App";
import { configureTheme, PreviewBridge } from "../preview/fixtures";
import allowlist from "../../scripts/i18n-allowlist.json";

afterEach(() => {
  cleanup();
  localStorage.clear();
  setPreviewLanguage("en");
  i18n.options.saveMissing = false;
  i18n.options.missingKeyHandler = false;
});

/** Pseudo text is bracketed; what is left outside the brackets came from somewhere else. */
const PSEUDO = /⟦[^⟧]*⟧/gu;
const WORD = /[A-Za-z][A-Za-z'’]+/gu;

/** Words a page shows that are neither copy nor data: formatted dates and key names. */
function formattedWords() {
  const words = new Set(["Ctrl", "Shift", "Alt", "Option", "Cmd", "Esc", "Enter", "Return", "Tab", "Space", "Delete", "Backspace", "AM", "PM"]);
  const add = (text: string) => { for (const word of text.match(WORD) ?? []) words.add(word); };
  for (let month = 0; month < 12; month += 1) for (const style of ["long", "short"] as const) add(new Intl.DateTimeFormat("en", { month: style }).format(new Date(2026, month, 3)));
  for (let day = 0; day < 7; day += 1) for (const style of ["long", "short"] as const) add(new Intl.DateTimeFormat("en", { weekday: style }).format(new Date(2026, 10, 1 + day)));
  const relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
  for (const unit of ["second", "minute", "hour", "day", "week", "month", "year"] as const) for (const value of [-2, -1, 0, 1, 2]) add(relative.format(value, unit));
  // Time zone names, as the Schedule header says them.
  for (const timeZone of Intl.supportedValuesOf("timeZone")) for (const timeZoneName of ["long", "short", "longGeneric"] as const) {
    const part = new Intl.DateTimeFormat("en", { timeZone, timeZoneName }).formatToParts(new Date(2026, 0, 15)).find(item => item.type === "timeZoneName");
    add(part?.value ?? "");
  }
  const currencies = new Intl.DisplayNames("en", { type: "currency" });
  for (const code of Intl.supportedValuesOf("currency")) add(currencies.of(code) ?? "");
  // A language's own name is never translated (the Appearance picker).
  for (const language of Object.keys(i18n.options.resources ?? {})) add(new Intl.DisplayNames(language, { type: "language" }).of(language) ?? "");
  // Product, provider and license names the copy check keeps as written.
  for (const entry of allowlist as { text: string }[]) add(entry.text);
  return words;
}

/** A bridge that remembers every string it hands the page, through results and streamed events. */
function recordingBridge(bridge: PreviewBridge) {
  const data: string[] = [];
  const seen = new WeakSet<object>();
  const keep = (value: unknown, depth = 0): void => {
    if (typeof value === "string") { data.push(value); return; }
    if (depth > 16 || !value || typeof value !== "object" || seen.has(value)) return;
    seen.add(value);
    for (const entry of Array.isArray(value) ? value : Object.values(value)) keep(entry, depth + 1);
  };
  const recorded = new Proxy(bridge, { get(target, key, receiver) {
    const value = Reflect.get(target, key, receiver);
    if (typeof value !== "function") return value;
    return (...args: unknown[]) => {
      const passed = args.map(arg => typeof arg === "function" ? (...events: unknown[]) => { events.forEach(event => keep(event)); return (arg as (...inner: unknown[]) => unknown)(...events); } : arg);
      const result = (value as (...inner: unknown[]) => unknown).apply(target, passed);
      if (result instanceof Promise) return result.then(resolved => { keep(resolved); return resolved; });
      keep(result);
      return result;
    };
  } });
  return { bridge: recorded, data };
}

/** Interface text outside the pseudo brackets that is not data the bridge sent: text that bypassed i18n. */
function bypassed(data: readonly string[], allowed: ReadonlySet<string>) {
  const sent = data.join("\n");
  const found = new Map<string, string>();
  const check = (text: string, where: string) => {
    for (const word of text.replace(PSEUDO, " ").match(WORD) ?? []) {
      if (!allowed.has(word) && !sent.includes(word) && !found.has(word)) found.set(word, `${where}: ${text.trim().slice(0, 120)}`);
    }
  };
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node.parentElement?.closest("script, style, svg")) continue;
    check(node.textContent ?? "", "text");
  }
  for (const attribute of ["aria-label", "aria-description", "title", "placeholder", "alt"]) {
    for (const element of document.querySelectorAll(`[${attribute}]`)) check(element.getAttribute(attribute) ?? "", attribute);
  }
  return [...found.values()];
}

const settle = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 300)); });
const press = async (element: Element | null, route: string) => {
  if (!element) throw new Error(`No control opens ${route}`);
  fireEvent.click(element);
  await settle();
};

/** Every page the shell can show, each reached the way a person reaches it. */
const routes: { name: string; open: () => Promise<void> }[] = [
  { name: "Bot", open: async () => {} },
  { name: "a research session", open: () => press(document.querySelector('[data-thread-id]:not([data-thread-id^="home-"])'), "a research session") },
  ...["following", "schedule", "map", "lens"].map(place => ({ name: place, open: () => press(document.querySelector(`[data-sidebar-place="${place}"] button`), place) })),
  { name: "Settings", open: () => press(document.getElementById("main-tab-settings"), "Settings") },
];

describe("the pseudo-locale", () => {
  it("renders every route with no missing key and no text that bypassed the catalogs", async () => {
    setPreviewLanguage("en-XA");
    const missing: string[] = [];
    i18n.options.saveMissing = true;
    i18n.options.missingKeyHandler = (_languages, namespace, key) => { missing.push(`${namespace}:${key}`); };
    const preview = new PreviewBridge({ delay_ms: 0 });
    await configureTheme(preview, "light");
    const { bridge, data } = recordingBridge(preview);
    render(<App bridge={bridge} />);
    await waitFor(() => expect(document.querySelector("[data-testid=app-shell]")).not.toBeNull(), { timeout: 8000 });
    await settle();
    const allowed = formattedWords();
    const report: string[] = [];
    const visit = async (name: string) => {
      for (const line of bypassed(data, allowed)) report.push(`${name} · ${line}`);
    };
    for (const route of routes) {
      await route.open();
      await visit(route.name);
      if (route.name !== "Settings") continue;
      // Each Settings section is its own page.
      const sections = [...document.querySelectorAll<HTMLElement>(".settings-navigation [data-sidebar=menu-button]")];
      expect(sections.length).toBeGreaterThan(3);
      for (const section of sections) {
        await press(section, "a Settings section");
        await visit(`Settings › ${section.textContent?.replace(PSEUDO, (text) => text).trim()}`);
      }
    }
    expect(missing, "keys no catalog has").toEqual([]);
    expect(report, "text that bypassed the catalogs").toEqual([]);
  }, 60_000);

  it("is generated from English: every English string has a bracketed, longer counterpart with its placeholders", () => {
    const english = i18n.getResourceBundle("en", "common") as Record<string, unknown>;
    const pseudo = i18n.getResourceBundle("en-XA", "common") as Record<string, unknown>;
    const pairs: [string, string][] = [];
    const walk = (left: unknown, right: unknown) => {
      if (typeof left === "string") { pairs.push([left, right as string]); return; }
      if (left && typeof left === "object") for (const [key, value] of Object.entries(left)) walk(value, (right as Record<string, unknown>)[key]);
    };
    walk(english, pseudo);
    expect(pairs.length).toBeGreaterThan(10);
    for (const [source, generated] of pairs) {
      expect(generated).toMatch(/^⟦[\s\S]*⟧$/u);
      for (const placeholder of source.match(/\{\{[^}]+\}\}/gu) ?? []) expect(generated).toContain(placeholder);
      const visible = source.replace(/\{\{[^}]+\}\}|<\/?\d+\s*\/?>/gu, "").length;
      if (visible >= 10) expect(generated.length - 2 - source.length).toBeGreaterThanOrEqual(Math.floor(visible * 0.4) - 1);
    }
  });
});

/**
 * Product code never branches on a language: it asks the catalogs, or a helper in
 * `src/i18n` that holds the locale's own rules (number words, date forms, particles).
 * Fixtures, the mock bridge, generated contracts and tests are not product code.
 */
const sources = import.meta.glob<string>(["../**/*.{ts,tsx}", "!../i18n/**", "!../preview/**", "!../bridge/mock/**", "!../test/**", "!../contracts/**", "!../**/*.test.{ts,tsx}"], { query: "?raw", import: "default", eager: true });
const KOREAN_BRANCH = /[=!]==?\s*["'`]ko(?:-[A-Za-z]+)?["'`]|["'`]ko(?:-[A-Za-z]+)?["'`]\s*[=!]==?|\.(?:startsWith|endsWith|includes)\(\s*["'`]ko\b|\/\^\(?(?:\?:)?ko\b|case\s+["'`]ko["'`]/u;
/** Each entry is a known branch waiting for a design decision; the reason says which. */
const KNOWN_BRANCHES: Record<string, string> = {
  "../components/assets/assetName.ts": "Names a Korean-listed asset by its Hangul Map name; doing this for every market's own script needs a market-to-language table (a product decision).",
};

it("finds no product code that branches on Korean", () => {
  expect(Object.keys(sources).length).toBeGreaterThan(100);
  const found = Object.entries(sources).flatMap(([file, text]) => text.split("\n").flatMap((line, index) =>
    KOREAN_BRANCH.test(line) && !(file in KNOWN_BRANCHES) ? [`${file.slice(3)}:${index + 1}: ${line.trim()}`] : []));
  expect(found).toEqual([]);
  // A known branch that is gone leaves the list too.
  for (const file of Object.keys(KNOWN_BRANCHES)) expect(sources[file]?.split("\n").some(line => KOREAN_BRANCH.test(line)), file).toBe(true);
});
