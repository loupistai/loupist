// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type AppMenuContext = {
  language: MenuLanguage,
  shell: boolean,
  loupist: boolean,
  print: boolean,
  sidebar: boolean,
  modal: boolean,
};

export type MenuLanguage = "en" | "ko";
