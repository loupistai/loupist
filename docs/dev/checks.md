# Checks and setup

The repository gate installs locked dependencies and compares the source tree before and after the checks.

## Broader checks

From `apps/loupist/`, `npm run check:web`, `check:rust` and `check:python` run
one scope without reinstalling dependencies; `check:harness` runs the
credential-free Pi/eval contracts also included in web. `npm run check` runs all
three scopes one after another (web → python → rust). Rust includes feature-gated
headless contracts without paid calls.

The web scope's `node-contracts` step includes root `scripts/*.test.mjs` contracts.
The `boundaries` step enforces renderer import boundaries with
`check-boundaries.mjs --enforce`.
The `unused-files` step rejects renderer files outside the current app, preview,
and external script import graph. See [Code cleanup](repository-guidance.md#code-cleanup).

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

Cloud agent VMs start without the pinned toolchains. `scripts/cloud-setup.sh`
installs the Linux build libraries, Node from `.nvmrc`, the Rust and uv versions
pinned in `.github/actions/setup-toolchains/action.yml`, then runs `setup.sh`. It
skips work already done and links the tools into `/usr/local/bin`, because cloud
snapshots keep files but not exported variables.

| Cloud agent | Wiring |
| --- | --- |
| Cursor Cloud Agents | `.cursor/environment.json` runs it as `install` once per environment build |
| Claude Code on the web | `.claude/settings.json` runs it at session start when `CLAUDE_CODE_REMOTE` is `true` |
| Codex cloud | Set the environment's setup script to `sh scripts/cloud-setup.sh` |

The script does not install Chrome, so the web gate's `chat-layout` step needs a
browser (`CHROME_PATH`) in a cloud session. Without one, run the other web steps
with `check.sh web --only <steps>` and leave layout to PR CI.

Project guidance follows the checked-out revision. Setup only installs dependencies.

### Linux cloud development

Linux can run the React preview, Node/Python contracts and the same Rust source
gate as macOS. Rust tests use the real persistence, Memory and process ownership
code without opening a window. The crate still links Tauri, so install its native
development libraries even on a host without a display.

Treat macOS and Linux as development hosts for shared code. Keep both source
gates working when changing platform adapters or their callers. Select native
validation separately from the host used to edit the code:

| Surface | macOS | Linux |
| --- | --- | --- |
| Shared renderer, Node/Python and Rust source behavior | Normal source gate | Same source gate with native build libraries |
| Fixed-state React appearance and layout | Browser preview | Browser preview |
| WKWebView, Keychain, OS dialogs/input and release acceptance | Native macOS suites | Qualify these changes on macOS |

Use [Which tool](../../apps/loupist/e2e/README.md#which-tool) to choose the
observable surface. A preview uses fixtures; it cannot qualify real persistence
or the native bridge. Rust source tests exercise the real backend independently.

Use the Node LTS version pinned in root `.nvmrc`, Python 3.12 with `uv`, and
CI's Rust 1.97.1 with `clippy` and `rustfmt`. On Debian/Ubuntu, install the host
packages once, then use the normal locked setup and verification commands from
the repository root:

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev librsvg2-dev libssl-dev libxdo-dev procps lsof
sh apps/loupist/scripts/setup.sh
scripts/verify.sh
```

If host package installation is unavailable and Docker is already available,
run these steps inside a disposable Debian/Ubuntu development container with
the pinned toolchains. Mount the checkout and install its locked npm and Python
environments inside the container; do not reuse a host `.venv` or mutable build
tree. Run build and check commands as the checkout's UID/GID (for example,
`docker run --user "$(id -u):$(id -g)" ...`); mixing root and host-owned build
commands can leave Tauri's generated files unwritable by the next check.
Keep the normal container procfs mount and PID namespace for process
ownership tests. In managed clouds, preserve the supplied network proxy and CA
trust when building and running the container.

The `rust` scope includes formatting, Clippy for normal and E2E builds, all-target
tests and the applicable `eval-harness` contracts. Narrow a development run with
`sh apps/loupist/scripts/check.sh rust`, or a filtered `cargo test` as described in
[Tests](tests.md#tests). Linux process leases require procfs access to verify the
boot, process start and executable identities; restricted or unverifiable owners
are refused. These checks make no paid model calls and need no Mac sidecar staging.

For interactive UI work, start Vite on the separate preview port:

```sh
npm --prefix apps/loupist run dev -- --port 1430
```

Open `http://localhost:1430/preview.html?scenario=chat-empty&theme=light&locale=ko`
in the cloud host's browser. The preview uses the real React app and deterministic
mock bridge data. For screenshot/layout checks, install Chrome or Chromium and
set `CHROME_PATH` to its executable before running `preview:capture` or the web
gate. See [UI review](ui-qa.md#desktop-design-system).

Pull-request CI runs Rust on Ubuntu; the `main` run adds macOS Rust, WKWebView/native
interaction and packaging, and releases add signing. Linux development
does not add a Linux release target. The Linux GTK dependency advisory remains
documented in [Dependency security review](dependencies.md#dependency-security-review).

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
