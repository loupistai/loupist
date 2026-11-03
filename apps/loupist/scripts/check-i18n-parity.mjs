#!/usr/bin/env node
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../src/i18n/locales");
const plural = /_(zero|one|two|few|many|other)$/;
const placeholders = /\{\{[\s\S]*?\}\}/g;
const tags = /<\/?\d+\s*\/?>/g;
const errors = [];

// Exact names, not generic English words: e.g. "Data Link" is allowed only
// inside the provider's full name. Korean particles may follow a Latin name.
const allowedTerms = new Map([
  ["Loupist", "Product name"],
  ["Loupist", "Product persona"],
  ["World", "Named knowledge feature"],
  ["Lens", "Named perspective feature"],
  ["Alpha Vantage", "Market data provider"],
  ["Anthropic", "Model provider"],
  ["OpenAI", "Model provider"],
  ["OpenRouter", "Model provider"],
  ["ChatGPT", "Model product"],
  ["Claude", "Model product"],
  ["SuperGrok", "Model product"],
  ["Benzinga", "Market data provider"],
  ["Biztoc", "News provider"],
  ["CFTC", "Source agency acronym"],
  ["Congress.gov", "Source provider and domain"],
  ["DART", "Source provider acronym"],
  ["DefiLlama", "Crypto data provider"],
  ["EconDB", "Economic data provider"],
  ["FRED", "Economic data provider acronym"],
  ["GitHub", "Service name"],
  ["Intrinio", "Market data provider"],
  ["Nasdaq Data Link", "Full data service name"],
  ["Nasdaq", "Provider name"],
  ["OpenBB", "Market data platform"],
  ["OpenDART", "Disclosure provider"],
  ["Tiingo", "Market data provider"],
  ["Tradier", "Market data provider"],
  ["Trading Economics", "Full economic data provider name"],
  ["World Bank", "Full source institution name"],
  ["Office", "Document product family"],
  ["Word", "Document product name"],
  ["PowerPoint", "Presentation product name"],
]);

// Technical literals and subscription names are exceptions only at the exact
// catalog key where they are required. Keep a reason with every exception.
const exceptions = new Map([
  ["settings.custom.pasteJson", { terms: ["JSON"], reason: "Literal data format the user pastes" }],
  ["settings.custom.commandWarning", { terms: ["name"], reason: "Literal package@version example (name@1.2.3)" }],
  ["app.navigation.agentDefaultsReasoningThinkingRetryTokensContextConfiguration", { terms: ["TOML"], reason: "Search keyword for the configuration file format" }],
  ["onboarding.appDialogs.chooseThisLoupistSOriginalOrCopiedWorkspace", { terms: [".loupist"], reason: "Literal workspace data directory name" }],
  ["knowledge.lensWorkspace.openALensCtrlO", { terms: ["Ctrl"], reason: "Keyboard shortcut must remain unchanged" }],
  ["settings.modelSettingsPanel.descriptionChatGpt", { terms: ["Plus", "Business", "Enterprise"], reason: "Official ChatGPT subscription names" }],
  ["settings.modelSettingsPanel.descriptionXai", { terms: ["Premium"], reason: "Official X subscription name" }],
  ["settings.modelSettingsPanel.customAdvancedBody", { terms: ["Ollama", "Studio", "vLLM"], reason: "Product names of compatible model servers" }],
  ["settings.modelSettingsPanel.customNamePlaceholder", { terms: ["Ollama"], reason: "Product name in an example provider name" }],
  ["settings.modelSettingsPanel.customApiOpenAi", { terms: ["Chat", "Completions"], reason: "Official name of the OpenAI Chat Completions API" }],
  ["settings.modelSettingsPanel.customApiAnthropic", { terms: ["Messages"], reason: "Official name of the Anthropic Messages API" }],
  ["settings.modelSettingsPanel.customBaseUrlHint", { terms: ["https", "http", "localhost"], reason: "Literal URL schemes and the loopback host name" }],
  ["settings.skillsSettings.chooseFolderDescription", { terms: ["SKILL.md"], reason: "Required skill filename" }],
  ["settings.skillsSettings.repairSkill", { terms: ["SKILL.md"], reason: "Required skill filename" }],
  ["settings.bundledSkills.documents.description", { terms: [".docx", ".dotx"], reason: "Literal supported file extensions" }],
  ["settings.bundledSkills.spreadsheets.description", { terms: [".xlsx", ".xlsm", "data-analysis"], reason: "Literal file extensions and skill identifier" }],
  ["settings.bundledSkills.presentations.description", { terms: [".pptx", ".potx"], reason: "Literal supported file extensions" }],
  ["connections.catalog.community_web_researchSummary", { terms: ["HTML"], reason: "Supported document format acronym" }],
  ["chat.browserWorkspaceView.enterAnHTTPOrHTTPSAddressAboveSearchQueries", { terms: ["HTTP", "HTTPS"], reason: "Accepted URL protocols" }],
  ["onboarding.wizard.pasteInvalid", { terms: ["http"], reason: "Literal loopback address the pasted sign-in address starts with" }],
  ["onboarding.wizard.portBusy", { terms: ["Codex"], reason: "Product name of the app that may hold the sign-in port" }],
  ["onboarding.wizard.localNote", { terms: ["Ollama", "Studio", "llama"], reason: "Product names of local model apps (LM Studio, llama.cpp)" }],
  ["settings.providers.localNone", { terms: ["Ollama", "Studio", "llama"], reason: "Product names of local model apps (LM Studio, llama.cpp)" }],
  ["settings.providers.addressHint", { terms: ["http", "localhost"], reason: "Literal example server address" }],
  ["settings.providers.testInvalid", { terms: ["https", "http", "localhost"], reason: "Literal URL schemes and the loopback host name" }],
  ["settings.providers.dialectResponses", { terms: ["Responses"], reason: "Official name of the OpenAI Responses API" }],
  ["settings.providers.dialectCompletions", { terms: ["Chat", "Completions"], reason: "Official name of the OpenAI Chat Completions API" }],
  ["settings.providers.dialectAnthropic", { terms: ["Messages"], reason: "Official name of the Anthropic Messages API" }],
  ["onboarding.wizard.symbolPlaceholder", { terms: ["AAPL"], reason: "Example ticker shown in the first-run symbol field" }],
  ["chat.workspaceTabBar.openAnHTTPOrHTTPSAddress", { terms: ["HTTP", "HTTPS"], reason: "Accepted URL protocols" }],
  ["sharing.showInFinder", { terms: ["Finder"], reason: "Name of the macOS file browser" }],
  ["chat.chatMessageCard.showInFinder", { terms: ["Finder"], reason: "Name of the macOS file browser" }],
  ["sharing.gist", { terms: ["Gist"], reason: "GitHub product name" }],
  ["sharing.gistNote", { terms: ["Gist"], reason: "GitHub product name" }],
  ["sharing.gistPublic", { terms: ["Gist"], reason: "GitHub product name" }],
  ["sharing.gistSecret", { terms: ["Gist"], reason: "GitHub product name" }],
  ["sharing.ghScope", { terms: ["Gist"], reason: "GitHub product name" }],
  ["sharing.linkHint", { terms: ["github.com", "Gist"], reason: "Accepted host name and GitHub product name" }],
  ["sharing.linkInvalid", { terms: ["gist.github.com", "github.com"], reason: "Accepted host names" }],
]);

function flatten(value, path = "", result = new Map()) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${path || "catalog"}: expected an object`);
  }
  if (path && !Object.keys(value).length) throw new Error(`${path}: empty nested object`);
  for (const [key, child] of Object.entries(value)) {
    if (!path && key === "_status") continue;
    const next = path ? `${path}.${key}` : key;
    if (typeof child === "string") result.set(next, child);
    else flatten(child, next, result);
  }
  return result;
}
const matches = (value, pattern) => [...value.matchAll(pattern)].map(([match]) => match).sort();
const same = (left, right) => JSON.stringify(left) === JSON.stringify(right);
const escape = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
function untranslatedWords(value, key) {
  let text = value.replace(placeholders, "").replace(tags, "");
  const local = exceptions.get(key);
  if (local && !local.reason.trim()) throw new Error(`Missing exception reason: ${key}`);
  const terms = [...allowedTerms.keys(), ...(local?.terms ?? [])].sort((a, b) => b.length - a.length);
  for (const term of terms) {
    text = text.replace(new RegExp(`(?<![A-Za-z0-9_])${escape(term)}(?![A-Za-z0-9_])`, "g"), "");
  }
  return [...new Set(text.match(/[A-Za-z]{4,}/g) ?? [])];
}
async function catalogs(locale) {
  const names = (await readdir(join(root, locale))).filter((name) => name.endsWith(".json")).sort();
  const result = new Map();
  for (const name of names) {
    const value = JSON.parse(await readFile(join(root, locale, name), "utf8"));
    if (locale !== "en" && value._status !== "translated") errors.push(`${locale}/${name}: _status must be translated`);
    result.set(name, flatten(value));
  }
  return result;
}

const english = await catalogs("en");
const locales = (await readdir(root, { withFileTypes: true }))
  .filter((entry) => entry.isDirectory() && entry.name !== "en").map((entry) => entry.name).sort();
let checked = 0;
for (const locale of locales) {
  const localized = await catalogs(locale);
  const categories = new Intl.PluralRules(locale).resolvedOptions().pluralCategories;
  for (const name of localized.keys()) {
    if (!english.has(name)) errors.push(`${locale}/${name}: extra namespace`);
  }
  for (const [name, source] of english) {
    const target = localized.get(name);
    if (!target) { errors.push(`${locale}/${name}: missing namespace`); continue; }
    const expected = new Set();
    const groups = new Set([...source.keys()].filter((key) => plural.test(key)).map((key) => key.replace(plural, "")));
    for (const key of source.keys()) {
      if (!plural.test(key)) expected.add(key);
    }
    for (const base of groups) for (const category of categories) expected.add(`${base}_${category}`);
    for (const key of expected) {
      if (!target.has(key)) errors.push(`${locale}/${name}:${key}: missing key`);
    }
    for (const [key, value] of target) {
      const base = key.replace(plural, "");
      const variant = plural.test(key) && groups.has(base);
      if (!source.has(key) && !variant) { errors.push(`${locale}/${name}:${key}: extra key`); continue; }
      const original = source.get(key) ?? source.get(`${base}_other`);
      if (original === undefined) { errors.push(`${locale}/${name}:${key}: plural group has no English other value`); continue; }
      const location = `${locale}/${name}:${key}`;
      if (!value.trim() && original.trim()) errors.push(`${location}: empty translation`);
      if (!same(matches(original, placeholders), matches(value, placeholders))) errors.push(`${location}: placeholder mismatch`);
      if (!same(matches(original, tags), matches(value, tags))) errors.push(`${location}: numbered tag mismatch`);
      if (locale === "ko" || locale.startsWith("ko-")) {
        const words = untranslatedWords(value, `${name.slice(0, -5)}.${key}`);
        if (words.length) errors.push(`${location}: untranslated English: ${words.join(", ")}`);
      }
      checked++;
    }
  }
}
if (errors.length) {
  for (const error of errors) console.error(error);
  console.error(`i18n parity: ${errors.length} error(s)`);
  process.exitCode = 1;
} else {
  console.log(`i18n parity: ${checked} translations in ${locales.length} locale(s), ${english.size} namespaces — OK`);
}
