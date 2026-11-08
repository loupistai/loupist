# Changelog

## Unreleased

- Show Loupist as up to date only after a check that found no newer release.
- Refuse an in-app update unless the signed package names the same version as the release feed.
- Make Lens a shared wiki for the user and the Loupist: Chat links open Lens, search is shared, drafts persist, conflicts merge or become proposals, and methods can be renamed, archived, compared, and viewed as a link graph.
- Limit the first supported release to macOS 13+ on Apple Silicon; retain Windows implementation and manual compatibility checks outside release gates.
- Upgrade 0.0.1 data in place after a private backup instead of refusing it, keep Keychain secrets reachable when an older development catalog is set aside, and refuse data a newer Loupist saved with a clear message.
- Fix Chats that could no longer be saved after a Loupist folder was reconnected or imported; the upgrade repairs affected Chats.
- Keep dataset export, dataset import and workspace capture to ordinary research files: they no longer reach the Loupist's private folder, git metadata or instruction files, and a fetched web page is never saved as the user's own material because of its site name.
- Replace the early Wiki model with Source, World, Lens, working notes, and a private Journal for each Loupist, with canonical writes in Rust.
- Add local workspaces for Pi's native file tools; keep app-owned Memory and Chat session data separate.
- Run custom Python analysis in a pinned WASI worker with no shell, network, process spawn or host environment.
- Replace the composer's three Memory modes with one Private chat switch that disables Memory reads and saves together.
- Add Scheduled Chat runs, inline execution progress, and side readers for files, sources, Memory, and published results.
- Keep isolated finance and compute services available alongside local tools for repeatable analysis.

## 0.0.1 - 2026-08-25

- Launch Loupist as a single local-first desktop product for macOS Apple Silicon and Windows x64.
- Add Wiki and Lens Skills that organize durable facts and set an original Decision against what actually happened.
- Remove host plugins, Packs, compatibility paths, and independent Core distribution.
- Add signed GitHub Releases and consent-based in-app updates.
- License the project under AGPL-3.0-only.
