# Development

This page owns local setup, development workflow, UI review and check selection.
Commands are repository-relative unless noted. Use Node 26.13+ LTS (26.x; `nvm install` reads the root `.nvmrc`), Python 3.12 with
`uv`, and the Rust toolchain pinned by CI. Install from lockfiles. Focused checks
assume dependencies already exist; setup and the full repository gate install them.
Web helper contracts also require `ps`, `pgrep` and `lsof`.

## Desktop work map

Paths below are relative to `apps/loupist/` unless noted. Open the leaf for the behavior; the code list on that leaf is the entry.

Trace UI through the bridge before adding a command. `src/bridge/tauriBridge.ts`, `src/contracts/`, `src-tauri/src/lib.rs`, `src-tauri/src/commands/`

- [Sidebar](ui/sidebar.md), [Schedule](ui/schedule.md), [Home](ui/home-thread.md), [Workspace panel](ui/workspace-panel.md)
- [Chat composer](ui/chat-composer.md), [Evidence and the browser](ui/evidence-browser.md), [Answer failures](ui/chat-answer.md)
- [Map and Lens](ui/map-lens.md), [Memory](knowledge/memory.md), [Asset pages](knowledge/asset-pages.md)
- [Chat turns](runtime/chat-turns.md), [Model providers](runtime/providers.md), [Storage and authority](runtime/storage-authority.md)
- [UI review](dev/ui-qa.md), [Tests](dev/tests.md), [Checks and setup](dev/checks.md), [CI and releases](ci-cd.md)

Keep bridge command names aligned with registered Rust handlers. Do not hand-edit
`src-tauri/gen/`, staged sidecars, `dist/`, `target/` or dependency directories.
Use [Architecture](architecture.md) for execution and credential boundaries.

## Find a topic

- [Maintaining development guidance](dev/repository-guidance.md)
- [Desktop design system](dev/ui-qa.md#desktop-design-system)
- [Design rubric](dev/ui-qa.md#design-rubric)
- [Chat reading column and in-app browser](ui/evidence-browser.md#chat-reading-column-and-in-app-browser)
- [Performance budgets](dev/ui-qa.md#performance-budgets)
- [Visual regression](dev/visual-regression.md#visual-regression)
- [Localization foundation](dev/ui-qa.md#localization-foundation)
- [Tests](dev/tests.md#tests)
- [Broader checks](dev/checks.md#broader-checks)
- [Diagnostics](dev/diagnostics.md#diagnostics)
- [Development loop](dev/checks.md#development-loop)
- [Work loops](dev/checks.md#work-loops)
- [Setup across development hosts](dev/checks.md#setup-across-development-hosts)
- [Generated storage](dev/checks.md#generated-storage)
- [Runtime staging and package checks](dev/checks.md#runtime-staging-and-package-checks)
- [Shared window](dev/checks.md#shared-window)
