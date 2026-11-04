# Checks and setup

The repository gate installs locked dependencies and compares the source tree before and after the checks.

## Broader checks

From `apps/loupist/`, `npm run check:web`, `check:rust` and `check:python` run
one scope without reinstalling dependencies; `check:harness` runs the
credential-free Pi/eval contracts also included in web. `npm run check` runs all
three scopes one after another (web → python → rust). Rust includes feature-gated
headless contracts without paid calls.

```sh
scripts/verify.sh
# Existing dependencies; --list prints the planned steps without running them:
sh apps/loupist/scripts/check.sh all
sh apps/loupist/scripts/check.sh rust --list
```

Agent harness changes use the
[failure taxonomy and adoption guard](../../apps/loupist/evals/README.md#failure-taxonomy-and-adoption-guard).
Adoption requires the deterministic boundary suite, non-decreasing regression
metrics, and a measured input-token check. The opt-in
[meta-harness](../../apps/loupist/evals/README.md#meta-harness-optimization)
proposes bounded prompt, Skill and tool-description edits, scores them on
development scenarios, and writes a report. It does not change the checkout or
run held-out suites. Keep those commands in the eval guide.

The repository gate installs locked dependencies. Per-checkout scope locks reject
overlapping runs, and a live native session in the same checkout blocks installation
and Rust checks. Direct commands remain outside those locks.

Each command has a private temporary directory and a default 30-minute timeout,
overridable with `--timeout`, and runs with `CARGO_INCREMENTAL=0`: the gate rebuilds a
sealed tree, so incremental caches only grow `target/` by several GB per run without
speeding it up. Cancellation stops its process group; failed steps
are not retried. Receipts and logs live in `apps/loupist/output/checks/`; runs
older than seven days are deleted on the next run. A pass requires the same Git
source tree id before and after the run; on a mismatch, `git diff-tree -r <before> <after>`
shows what changed. A receipt does not qualify later edits. Keep credentials out of
logs and review artifacts before sharing.

### Development loop

Start with the observable result and affected surfaces. Reproduce the narrow case,
make an attributable correction and rerun it before broadening. Distinguish product,
fixture, environment and variable-model failures; preserve their evidence in reports.
Missing credentials/platforms and unapproved external costs are boundaries, not
passes. Never change a rubric merely to make a candidate pass.

### Work loops

Larger efforts run as bounded loops. Every loop has an iteration cap, a no-progress check, and a cost cap,
and it never edits its own stop condition or its graders.

| Loop | Trigger | Steps | Stop when | Escalate when |
| --- | --- | --- | --- | --- |
| Build | each work item | spec → implement → narrow tests → capture affected states → independent review → itemized fixes | acceptance checks pass and review has no P0/P1 findings | 3 fix rounds on one spec (re-scope it) or 5 rounds without progress |
| Design review | each screen or flow change, and phase end | capture the state matrix → review every cell against its design spec and the rubric → fix → update baselines | every cell passes | a trade-off would change a fixed product decision |
| Regression | before handoff and at phase end | `scripts/verify.sh` + credential-free eval tiers + regression suites | green | a failure appears outside the change |
| Capability | phase end, within the eval budget | capability suites → trace analysis → harness change | baseline recorded | the budget would be exceeded |
| Harness optimization | after regression, capability, and held-out suites exist | propose → train → validate → held-out once → adopt or reject | a candidate is adopted or the cap is reached | a held-out regression or a gaming signal |

Review is done by a separate pass that did not build the change. A check repeated by hand twice becomes a
script, test, or eval.

### Setup across development hosts

Codex's `.codex/environments/environment.toml` and Cursor's `.cursor/worktrees.json`
call `sh apps/loupist/scripts/setup.sh`; use the same command for any other
worktree host. Setup installs app/E2E npm and worker uv lockfiles and fetches Cargo
dependencies; it does not claim verification.

Project guidance follows the checked-out revision. Setup only installs dependencies.

### Generated storage

Development Rust profiles use `debug = "line-tables-only"`; full variable/type
debugging requires an explicit full-debug override. Share download caches, not mutable
Cargo targets, `node_modules` or worker `.venv` directories across active checkouts.
Check runs prune their own old receipts; other build caches are removed explicitly
after stopping their owners. Never sweep user workspaces or credentials as check output.

Keep screenshots, recordings, traces and machine-specific reports under the ignored
root `output/` or `apps/loupist/output/` directory. Review any artifact before moving
it into shared documentation. Build output, Python/Vite caches and Tauri's
`src-tauri/gen/` schemas are disposable; `tauri-build` regenerates the schemas during
the native build. App assets, embedding models and golden fixtures are maintained
inputs and remain versioned. Eval baseline reports stay local and Git-ignored.
Delete known output paths
explicitly; a blanket ignored-file cleanup would also delete private notes in
`docs/local/` and installed development dependencies.

## Runtime staging and package checks

Stage missing/stale runtimes before a requested native or packaging flow.
World extraction and search run in Rust; only their local embedding assets need
staging. Pi staging verifies its pinned archive and executable regressions.
Keep the Pi package family on the same exact version and follow the pins in
`scripts/pi-security.json` when updating dependencies.
Dependency security floors and platform-specific findings are recorded in
[dependency review](dependencies.md#dependency-security-review). The native
dependency set requires Rust 1.90 or newer; use CI's pinned toolchain.

```sh
python3 apps/loupist/scripts/stage-embed-assets.py
# Complete platform staging also includes embeddings:
sh apps/loupist/scripts/stage-macos-arm64.sh
# Windows compatibility staging:
pwsh apps/loupist/scripts/stage-windows-x64.ps1
```

After a bundle change use the bundle checks in [CI and releases](../ci-cd.md).
Do not commit staged binaries, build output, credentials, databases or attachments.

## Shared window

Before launching/inspecting the native app, read
[Which tool](../../apps/loupist/e2e/README.md#which-tool). It owns Vitest/headless
eval/WebDriver/computer-use selection and isolated app-data profiles. Mock-bridge
previews establish renderer appearance, not native behavior. Prompt/tool changes
also use [task verification](../agent/prompt.md#verify-the-users-completed-task).
