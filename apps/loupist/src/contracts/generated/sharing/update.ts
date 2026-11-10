// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ItemView } from "../sharing/import";
import type { Flag } from "../sharing/scan";

export type ChangedItem = {
  key: string,
  kind: string,
  title: string,
  /** `pending` (still a suggestion) or `accepted`. */
  state: string,
  /** Accepted, then changed here: theirs is shown beside yours. */
  edited: boolean,
  flags: Array<Flag>,
  /** The text as it stands here (yours). */
  then: string,
  /** The author's newer text, validated and scanned. */
  now: ItemView,
};

export type MapChanges = {
  names: number,
  links: number,
  withdrawn: number,
  /** The import's map is already here (otherwise applying adds the whole map). */
  added: boolean,
};

export type NewItem = {
  key: string,
  kind: string,
  title: string,
  flags: Array<Flag>,
  item: ItemView,
};

export type PackUpdate = {
  stagingId: string,
  importId: string,
  fromVersion: number,
  toVersion: number,
  createdAt: string,
  newItems: Array<NewItem>,
  changed: Array<ChangedItem>,
  removed: Array<RemovedItem>,
  map: MapChanges | null,
  blocked: string | null,
};

export type RemovedItem = {
  key: string,
  kind: string,
  title: string,
  /** The accepted record it became: kept here, and the person may archive it. */
  lensRecordId: string,
};
