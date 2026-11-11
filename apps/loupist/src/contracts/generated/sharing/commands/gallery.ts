// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { Entry } from "../../sharing/gallery";

export type GalleryImageRequest = { entryId: string };

export type GalleryIndexRequest = { refresh?: boolean };

export type GalleryListingRequest = {
  loupistId: string,
  acceptedTerms?: boolean,
  termsUrl?: string | null,
};

export type GalleryView = {
  /** `ready`, `offline` (the kept copy), `unavailable` (none kept) or `notConfigured`. */
  status: string,
  fetchedAtMs: number | null,
  entries: Array<Entry>,
};
