import i18n from "@/i18n";
import type { KnowledgePin } from "../contracts/knowledge";
import type { ChatArtifactRef } from "../contracts/chat";
import { serializeInlineReferences, validInlineReferences, type ComposerInlineReference } from "./composerInlineReferences";

import type { ComposerChip } from "../contracts/workspace";
export type { ComposerChip } from "../contracts/workspace";

const t = i18n.getFixedT(null, "chat");

export const composerChipInlineText = (chip: ComposerChip) => chip.kind === "quote"
  ? `“${chip.text ?? chip.label}”` : `@${chip.label}`;

const inlineChipSubmission = (chip: ComposerChip) => {
  if (chip.kind === "connection") return `@${chip.id}`;
  if (chip.kind === "file") return chip.id;
  if (chip.kind === "quote") return `“${chip.text ?? chip.label}”`;
  return `$${chip.id}`;
};

const QUOTE_LABEL_CHARS = 40;
/** Quotes longer than this are cut so one selection cannot crowd out the question. */
export const MAX_QUOTE_CHARS = 2_000;

/** A quote chip from text selected in an answer (or a reader), keyed by its source and text. */
export function quoteChip(sourceId: string, selected: string): ComposerChip {
  const text = selected.replace(/\s+/gu, " ").trim().slice(0, MAX_QUOTE_CHARS);
  const short = text.length > QUOTE_LABEL_CHARS ? `${text.slice(0, QUOTE_LABEL_CHARS).trimEnd()}…` : text;
  return { key: `quote:${sourceId}:${text}`, kind: "quote", id: sourceId, label: `“${short}”`, text };
}

export type ContextOption = {
  id: string;
  group: "Files" | "Lens" | "World" | "Sources" | "Research" | "Documents" | "Connections";
  label: string;
  detail: string;
  pin?: KnowledgePin;
  path?: string;
  artifact?: ChatArtifactRef;
  connectionId?: string;
};

/** Context groups are stable internal identifiers; only their display labels are localized. */
export const contextGroupLabel = (group: ContextOption["group"]): string => ({
  Files: t("searchComposerContext.files"),
  Sources: t("searchComposerContext.sources"),
  Research: t("searchComposerContext.research"),
  Documents: t("searchComposerContext.documents"),
  Connections: t("searchComposerContext.connections"),
  World: "World",
  Lens: "Lens",
})[group];

export const rankText = (query: string, fields: readonly string[]) => {
  const needle = query.trim().toLowerCase();
  if (!needle) return 1;
  const values = fields.map((field) => field.toLowerCase());
  if (values.some((value) => value.startsWith(needle))) return 4;
  if (values.some((value) => value.split(/[\s._:/-]+/u).some((word) => word.startsWith(needle)))) return 3;
  if (values.some((value) => value.includes(needle))) return 2;
  return 0;
};

/** Turn display chips into the tokens and paths the existing Chat contract already accepts. */
export const composeSubmission = (prompt: string, chips: readonly ComposerChip[] = [], references: readonly ComposerInlineReference[] = []) => {
  const inline = validInlineReferences(prompt, references);
  const inlineKeys = new Set(inline.map(reference => reference.key));
  const byKey = new Map(chips.map(chip => [`chip:${chip.key}`, chip]));
  const body = serializeInlineReferences(prompt, inline, key => {
    const chip = byKey.get(key);
    if (chip) return inlineChipSubmission(chip);
    const reference = inline.find(reference => reference.key === key);
    // Knowledge/artifact ownership travels in typed fields; prose uses the human label.
    return reference?.text.startsWith("⟦") && reference.text.endsWith("⟧") ? reference.text.slice(1, -1) : undefined;
  }).trim();
  const prefixes: string[] = [];
  const quotes: string[] = [];
  for (const chip of chips) {
    if (inlineKeys.has(`chip:${chip.key}`)) continue;
    if (chip.kind === "quote") {
      if (chip.text) quotes.push(chip.text.split("\n").map((line) => `> ${line}`).join("\n"));
    } else if (chip.kind === "skill") {
      const token = `$${chip.id}`;
      if (!body.includes(token)) prefixes.push(token);
    } else if (chip.kind === "connection") {
      const token = `@${chip.id}`;
      if (!body.includes(token)) prefixes.push(token);
    } else if (chip.id && !body.includes(chip.id)) {
      prefixes.push(t("composerChips.filePrompt", { id: chip.id }));
    }
  }
  const asked = body || (quotes.length ? t("composerChips.explainQuote") : "");
  return [...quotes, prefixes.join(" "), asked].filter(Boolean).join("\n\n");
};

export const mentionLabel = (
  token: string,
  skills: ReadonlyArray<{ id: string; name: string }>,
  plugins: ReadonlyArray<{ id: string; name: string }>,
) => {
  if (token.startsWith("$")) return skills.find((skill) => `$${skill.id}` === token)?.name;
  if (token.startsWith("@")) return plugins.find((plugin) => `@${plugin.id}` === token)?.name;
  return undefined;
};
