# Development

This page owns local setup, development workflow, UI review and check selection.
Commands are repository-relative unless noted. Use Node 26.13+ LTS (26.x; `nvm install` reads the root `.nvmrc`), Python 3.12 with
`uv`, and the Rust toolchain pinned by CI. Install from lockfiles. Focused checks
assume dependencies already exist; setup and the full repository gate install them.
Web helper contracts also require `ps`, `pgrep` and `lsof`.

## Desktop work map

Paths below are relative to `apps/loupist/` unless noted. Open the leaf for the behavior; the code list on that leaf is the entry.

Trace UI through the bridge before adding a command. `src/bridge/tauriBridge.ts`, `src/contracts/`, `src-tauri/src/lib.rs`, `src-tauri/src/commands/`

- [Sidebar](ui/sidebar.md), [Schedule](ui/schedule.md), [Home Thread](ui/home-thread.md), [Workspace panel](ui/workspace-panel.md)
- [Chat composer](ui/chat-composer.md), [Evidence and the browser](ui/evidence-browser.md), [Answer failures](ui/chat-answer.md)
- [Map and Lens](ui/map-lens.md), [Memory](knowledge/memory.md), [Asset pages](knowledge/asset-pages.md)
- [Chat turns](runtime/chat-turns.md), [Model providers](runtime/providers.md), [Storage and authority](runtime/storage-authority.md)
- [UI review](dev/ui-qa.md), [Tests](dev/tests.md), [Checks and setup](dev/checks.md), [CI and releases](ci-cd.md)

Keep bridge command names aligned with registered Rust handlers. Do not hand-edit
`src-tauri/gen/`, staged sidecars, `dist/`, `target/` or dependency directories.
Use [Architecture](architecture.md) for execution and credential boundaries.

## Maintaining development guidance

[Repository guidance](dev/repository-guidance.md) owns discovery, decisions, development procedures and session handoff.

## Desktop design system

Tokens, components and the review rubric. [UI review](dev/ui-qa.md#desktop-design-system).

### Design rubric

What a first-time user would notice, in both themes and at the minimum window. [UI review](dev/ui-qa.md#design-rubric).

### Chat reading column and in-app browser

Column width, the rail breakpoint, and browser ready/blocked/failure. [Evidence and the browser](ui/evidence-browser.md#chat-reading-column-and-in-app-browser).

### Performance budgets

Native paint, switch and map-draw budgets. A miss is recorded; a missing shell fails. [UI review](dev/ui-qa.md#performance-budgets).

#### Visual regression

Capture, diff and baseline acceptance. [Visual regression](dev/visual-regression.md#visual-regression).

### Localization foundation

English and Korean catalogs, and the preview-only pseudo-locale. [UI review](dev/ui-qa.md#localization-foundation).

## Tests

Commands and the filter index. [Tests](dev/tests.md#tests).

## Diagnostics

Content-free local run logs. [Diagnostics](dev/diagnostics.md#diagnostics).

## Broader checks

Locked gates, loops and worktree setup. [Checks and setup](dev/checks.md#broader-checks).

### Development loop

Reproduce the narrow case and rerun it before widening. [Checks and setup](dev/checks.md#development-loop).

### Work loops

Build, design review, regression, capability and harness loops, each with a stop. [Checks and setup](dev/checks.md#work-loops).

### Setup across development hosts

Worktree setup installs dependencies from lockfiles. [Checks and setup](dev/checks.md#setup-across-development-hosts).

### Generated storage

Share download caches, not mutable build trees. [Checks and setup](dev/checks.md#generated-storage).

## Runtime staging and package checks

Stage pinned Pi and embedding assets before a native or packaging run. [Checks and setup](dev/checks.md#runtime-staging-and-package-checks).

## Shared window

Which native tool to run, and the isolated app-data profile. [Checks and setup](dev/checks.md#shared-window).
