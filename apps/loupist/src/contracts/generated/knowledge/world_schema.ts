// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type ExtensionCategory = "company" | "person" | "product" | "market" | "event" | "other";

export type ExtensionField = { name: string, description: string };

export type ExtensionLabels = { en: string, ko?: string };

export type ExtensionRelation = {
  name: string,
  description: string,
  signatures: Array<[string, string]>,
  labels: ExtensionLabels,
};

export type ExtensionType = {
  name: string,
  description: string,
  category: ExtensionCategory,
  labels: ExtensionLabels,
  /** Short user-facing line. `description` stays the English model-facing definition. */
  blurbs: ExtensionLabels,
  fields: Array<ExtensionField>,
};

export type KindLabel = { name: string, labels: ExtensionLabels };

export type SchemaChangeView = {
  changeId: string,
  producer: string,
  summary: string,
  addedTypes: Array<string>,
  addedRelations: Array<string>,
  removedTypes: Array<string>,
  removedRelations: Array<string>,
  kindLabels: Array<KindLabel>,
  createdAtMs: number,
  undoable: boolean,
};

export type WorldExtension = {
  version: number,
  types: Array<ExtensionType>,
  relations: Array<ExtensionRelation>,
  updatedAtMs: number,
};

export type WorldSchemaView = { extension: WorldExtension, changes: Array<SchemaChangeView> };
