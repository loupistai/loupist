import i18n from "@/i18n";
import { resources } from "@/i18n";

/** Tool names come from the catalog at runtime, so the key is not one of the typed literals. */
const tDynamic = i18n.getFixedT(null, "chat") as unknown as (key: string, options?: Record<string, unknown>) => string;

type StepLabels = Record<string, { running: string; done: string }>;

/** The catalog's English step labels (kept equal to `agent/tool-catalog.json` by `progressLabels.test.ts`). */
const englishLabels = (resources.en.chat as { toolProgress: StepLabels }).toolProgress;

/**
 * A tool step's own words in the interface language: the catalog's label for
 * its tool, whatever the host wrote, so a Korean interface never shows the
 * host's English mid-run ("Reading hanul.example"). Undefined for a step whose
 * tool the catalog does not have; its host wording then stands.
 */
export function catalogStepLabel(tool: string | undefined, form: "running" | "done"): string | undefined {
  const entry = tool && Object.hasOwn(englishLabels, tool) ? englishLabels[tool] : undefined;
  if (!entry) return undefined;
  return tDynamic(`toolProgress.${tool}.${form}`, { defaultValue: entry[form] });
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
