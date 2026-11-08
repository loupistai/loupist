# Changelog

## Unreleased

- Show Loupist as up to date only after a check that found no newer release.
- Clear a failed update check once a later check succeeds.
- Refuse an in-app update unless the signed package names the same version as the release feed.
- Make Lens a shared wiki for the user and the Loupist: Chat links open Lens, search is shared, drafts persist, conflicts merge or become proposals, and methods can be renamed, archived, compared, and viewed as a link graph.
- Limit the first supported release to macOS 13+ on Apple Silicon; retain Windows implementation and manual compatibility checks outside release gates.

- Replace the early Wiki model with Source, World, Lens, working notes, and a private Journal for each Loupist, with canonical writes in Rust.
- Add user-selected local workspaces for Pi's native file and Bash tools; keep app-owned Memory and Chat session data separate.
- Add Scheduled Chat runs, inline execution progress, and side readers for files, sources, Memory, and published results.
- Keep isolated finance and compute services available alongside local tools for repeatable analysis.

## 0.0.1 - 2026-08-25

- Launch Loupist as a single local-first desktop product for macOS Apple Silicon and Windows x64.
- Add Wiki and Lens Skills that organize durable facts and set an original Decision against what actually happened.
- Remove host plugins, Packs, compatibility paths, and independent Core distribution.
- Add signed GitHub Releases and consent-based in-app updates.
- License the project under AGPL-3.0-only.
