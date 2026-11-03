import i18n from "@/i18n";
import { resources } from "@/i18n";

/** Tool names come from the catalog at runtime, so the key is not one of the typed literals. */
const tDynamic = i18n.getFixedT(null, "chat") as unknown as (key: string, options?: Record<string, unknown>) => string;

type StepLabels = Record<string, { running: string; done: string }>;

/** The catalog's English step labels (kept equal to `agent/tool-catalog.json` by `progressLabels.test.ts`). */
const englishLabels = (resources.en.chat as { toolProgress: StepLabels }).toolProgress;

/**
 * A step label in the interface language. A tool step names its tool; when the
 * host used that tool's catalog wording, the translated entry replaces it.
 * Labels the host composed itself ("Opened Morrow research tools", "Reading
 * hanul.example") and steps without a tool name pass through unchanged.
 */
export function localizedProgressLabel(label: string, tool: string | undefined): string {
  const entry = tool && Object.hasOwn(englishLabels, tool) ? englishLabels[tool] : undefined;
  if (!entry) return label;
  const form = label === entry.running ? "running" : label === entry.done ? "done" : undefined;
  return form ? tDynamic(`toolProgress.${tool}.${form}`, { defaultValue: label }) : label;
}

/**
 * What a read was about, as a noun ("market data", "daily prices for 990221"),
 * for as-of notes and refused step rows. Connection tools may arrive with a
 * server prefix (`mcp__openbb__equity_profile`); an unknown tool falls back to
 * its connection's name, then to its own words.
 */
export function readNoun(tool: string, options: { target?: string; connection?: string } = {}): string {
  const name = tool.split("__").at(-1) ?? tool;
  const known = i18n.exists(`chat:asOf.reads.${name}`);
  const read = known ? tDynamic(`asOf.reads.${name}`) : options.connection?.trim() || name.replaceAll("_", " ");
  return options.target ? tDynamic("asOf.readFor", { read, target: options.target }) : read;
}
