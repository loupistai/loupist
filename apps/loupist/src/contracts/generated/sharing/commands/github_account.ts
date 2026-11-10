// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type GithubAccount = {
  clientConfigured: boolean,
  indexConfigured: boolean,
  galleryTermsUrl: string | null,
  login: string | null,
};

export type SignInEvent =
  | { "kind": "code", userCode: string, expiresInSec: number }
  | { "kind": "waiting" }
  | { "kind": "signed_in", login: string }
  | { "kind": "failed", reason: string };
