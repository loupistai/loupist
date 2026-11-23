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
 * A data read whose tool has a localized noun (a bundled market-data read such as
 * `mcp__openbb__equity_fundamental_income`): "Reading income statements" /
 * "손익계산서 읽는 중", so the host's English tool wording never reaches a Korean
 * live line. Undefined for a tool without a known noun.
 */
export function readStepLabel(tool: string | undefined, form: "running" | "done"): string | undefined {
  const read = readNounFor(tool?.split("__").at(-1));
  return read ? tDynamic(`readStep.${form}`, { read }) : undefined;
}

/**
 * What a read was about, as a noun ("market data", "daily prices for 990221"),
 * for as-of notes and refused step rows. Connection tools may arrive with a
 * server prefix (`mcp__openbb__equity_profile`); an unknown tool falls back to
 * its connection's name, then to its own words.
 */
export function readNoun(tool: string, options: { target?: string; connection?: string } = {}): string {
  const name = tool.split("__").at(-1) ?? tool;
  const read = readNounFor(name) ?? (options.connection?.trim() || name.replaceAll("_", " "));
  return options.target ? tDynamic("asOf.readFor", { read, target: options.target }) : read;
}

/** A data read's noun ("current quote"): the as-of catalog first, then the sheet's own. */
export function readNounFor(name: string | undefined) {
  if (!name) return undefined;
  if (i18n.exists(`chat:asOf.reads.${name}`)) return tDynamic(`asOf.reads.${name}`);
  if (i18n.exists(`chat:activitySheet.reads.${name}`)) return tDynamic(`activitySheet.reads.${name}`);
  return undefined;
}

/** A known research activity shared by progress and retained Source metadata. */
export function knownActivityLabel(tool: string | undefined, form: "running" | "done") {
  const name = tool?.split("__").at(-1);
  if (name && i18n.exists(`chat:activitySheet.tools.${name}`)) return tDynamic(`activitySheet.tools.${name}.${form}`);
  const catalog = catalogStepLabel(tool, form);
  if (catalog) return catalog;
  const noun = readNounFor(name);
  if (noun) return tDynamic(`readStep.${form}`, { read: noun });
}
