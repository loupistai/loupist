import manifest from "../../provider-manifest.json";

/**
 * The renderer's slice of the bundled provider manifest that Rust also reads
 * (`providers/manifest.rs`): which providers are featured, in what order, and
 * the product names people know them by. Whether a provider is connectable
 * still comes from the catalog Rust sends.
 */
type ManifestProvider = {
  id: string;
  group?: string;
  names?: { key?: string; sign_in?: string };
  api_key?: { featured_row?: number };
};

const providers: readonly ManifestProvider[] = manifest.providers;

/** Sign-in rows and onboarding's sign-in buttons, in manifest order. */
export const SIGN_IN_PROVIDERS: readonly string[] = providers.filter(provider => provider.group === "sign_in").map(provider => provider.id);

/** Featured API-key rows by `featured_row`; presets such as Upstage Solar are rows too. */
export const FEATURED_KEY_ROWS: readonly string[] = [
  ...providers.flatMap(provider => provider.api_key?.featured_row ? [{ id: provider.id, row: provider.api_key.featured_row }] : []),
  ...manifest.presets.map(preset => ({ id: preset.id, row: preset.featured_row })),
].sort((left, right) => left.row - right.row).map(entry => entry.id);

const names = (pick: (provider: ManifestProvider) => string | undefined): Readonly<Record<string, string>> =>
  Object.fromEntries(providers.flatMap(provider => { const name = pick(provider); return name ? [[provider.id, name]] : []; }));

export const SIGN_IN_NAMES = names(provider => provider.names?.sign_in);
export const KEY_NAMES: Readonly<Record<string, string>> = {
  ...names(provider => provider.names?.key),
  ...Object.fromEntries(manifest.presets.map(preset => [preset.id, preset.name])),
};

const UPSTAGE_BASE_URL = manifest.presets.find(preset => preset.id === "upstage")?.base_url;

/** Whether a saved custom provider is the Upstage Solar preset, by its fixed address. */
export const isUpstageEndpoint = (baseUrl: string) => UPSTAGE_BASE_URL !== undefined && baseUrl.startsWith(UPSTAGE_BASE_URL);
