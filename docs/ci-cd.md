# CI and releases

First-release target: macOS 13+ on Apple Silicon only. Windows, Intel Macs and Linux are not supported in this release. Repository identity is `loupistai/loupist`.

This page owns GitHub settings, CI and release gates. Version tags and the manual update qualification are in [Releases](release.md). Workflows and scripts in the repo are the source of truth for smoke details.

## GitHub

The official repository is `loupistai/loupist` under the Loupist AI organization.
Organization members require two-factor authentication. Base repository permissions
are `none`; grant access explicitly per repository or team. Organization owners
control repository visibility changes, deletion, transfer and GitHub App installation.

`main` is the integration and release base. Work arrives through short-lived
branches named `<tool>/<topic>` (for example `codex/`, `cursor/` or `claude/`) and
pull requests. Use squash merges, delete merged branches, require resolved
conversations, and apply the same `main` rules to administrators. A pull request
needs only the `Source and product contracts` check and does not have to be current
with `main`; the `main` push run verifies the merged result. Auto-merge is on, so
`gh pr merge --auto --squash` lands a PR when that check passes. The
single-maintainer setup requires zero approving reviews. CODEOWNERS routes
reviews; add a review-count requirement when another maintainer can provide it.

Before removing a required check, list pull requests with auto-merge armed
(`gh pr list --json number,autoMergeRequest`). Removing the check merges them at once.

Keep the default workflow token read-only, disable workflow-created approvals,
and require full commit-SHA pins for third-party Actions. Approve workflows from
outside contributors before they run. Weekly grouped Dependabot PRs cover Actions,
npm, Cargo and uv; review them through the ordinary CI and PR process. The Actions
update entry also lists the shared toolchain action's directory, so one grouped PR
moves the pins in workflows and in both composite actions under `.github/actions/`. Dependabot
accepts only ruff patch updates, because `openbb-core` requires ruff below 0.16.
It skips `@earendil-works/*` version updates: a Pi upgrade moves the package
family, security pins and signed sidecar digests together by hand
([dependency review](dev/dependencies.md#dependabot-review-2026-11-04)). Private
vulnerability reporting, Dependabot alerts/updates, secret scanning and push
protection belong in the repository settings.

OpenBB and its extensions receive grouped minor/patch updates. Major upgrades are
coordinated runtime changes: migrate the base package, extensions, manifest and
sidecar contracts together rather than proposing incompatible individual majors.

CodeQL default setup analyzes Actions, JavaScript/TypeScript and Python on GitHub.
Rust is checked by the Rust CI job's formatting, Clippy and behavior/security tests.

Release, qualification and promotion must be enabled by the maintainer after
signing setup and the read-only audit, before pushing the first tag. A disabled
workflow does not run when a tag is pushed. CI package smokes publish only
short-lived workflow evidence, not GitHub Releases or update manifests.

Require pull requests for `main`. Restrict `v*` tag creation, update and deletion.
Protect `release`, `release-qualification`, and `stable-release` with custom `v*`
tag deployment policies. Signing secrets live only in `release`. The unprotected
`release-dry-run` environment must exist with no secrets, reviewers or deployment
restrictions so branch rehearsals work without real credentials.
`stable-release` requires a reviewer and disallows administrator bypass. While
exactly one collaborator has admin or maintain rights, it permits that maintainer
to approve their own promotion. This is self-review, with no second person; the
audit prints `WARN`. When another maintainer joins, restore
`prevent_self_review: true`; the audit rejects self-review with multiple maintainers.
Enable immutable GitHub Releases.

Before every RC tag and stable promotion, run the checked-in, read-only audit
from an authenticated GitHub CLI session:

```sh
python3 apps/loupist/scripts/check-github-release-setup.py
```

It verifies the public repository identity, immutable releases, `main`
pull-request protection, an active `v*` tag rule that restricts creation,
updates, and deletion, each required protected environment, and the **names** of every
secret required by the checked-in release profile. The updater secrets and the
selected mode's credentials must be present only in the
`release` environment, never at repository scope or in any non-signing
environment. The other signing mode's names may be absent. The union of updater
and per-mode secret sets must match every secret reference in `release.yml`. It never prints or reads secret values and does not change GitHub
state. A nonzero result names every missing item. The workflow-reference drift guard prevents a new signing secret from silently
escaping the checklist.

The immutable-release endpoint requires repository administration read access.
Release workflows intentionally do not receive an administrative token, so the
maintainer's authenticated, read-only audit is the release gate for that setting
rather than adding a broader long-lived credential to every release job.

## CI

Pull requests run shared web, Python and Rust contracts on Linux, selected by
changed paths. They never wait for a macOS runner. Linux WebKitGTK UI validation
is available on demand for cloud-development changes; it is not repeated on every
PR and main push. The `main` push run, manual
`workflow_dispatch` and the release call also run the macOS Rust contracts, native
interaction and package smoke for the selected scopes. To run the macOS jobs on a
branch before merging, use `gh workflow run ci.yml --ref <branch>`; a dispatch runs
all source and macOS checks. Add `-f linux_native=true` to also exercise Linux
WebKitGTK interaction and persistence under Xvfb. Every run's first job runs
`check.sh repository`: guide links, scope regression tests, cloud setup syntax,
credential boundaries and Node pins, without installing npm, OpenBB or Rust
dependencies. `scripts/verify.sh --affected` runs the same scope and classifier
locally ([handoff gate](dev/checks.md#broader-checks)). The Linux Rust checks do not
need a display.

| Changed surface | Selected checks beyond the always-on repository checks |
| --- | --- |
| README and ordinary guides | None |
| Renderer code or styles | Web and native interaction |
| Renderer/agent JavaScript unit tests | Web |
| Rust test modules or the headless harness | Rust |
| OpenBB tests or Python script tests | Python |
| Production Rust | Rust, native interaction and packaging |
| OpenBB runtime | Python, Rust, native interaction and packaging |
| Agent instructions/Skills or Marketplace assets | Web, Rust, native interaction and packaging |
| App dependencies, toolchain files, workflows, staging scripts, or unknown paths | All |

Mixed changes take the union of their scopes. Shared renderer contract fixtures
also select Rust, and agent eval fixtures select Web and Rust. Inline Rust tests
in production files retain native/package checks. PRs use a merge-base diff;
`main` pushes diff from the last successful `main` push run. Deleted and renamed source paths
remain in scope. Missing history or classification errors select all checks.
Native interaction runs on macOS outside pull requests and on Linux only for an
explicit `workflow_dispatch` with `linux_native=true`. Automatic Linux Rust tests
remain required: they exercise shared persistence, process ownership and worker
isolation on the host used by cloud agents. Packaging runs only outside pull requests.

Rust, uv and Ubuntu package caches are saved only by `main` runs; pull requests restore
them without uploading branch copies. The signed build shares the package-smoke
dependency cache. Official package CI has one explicit macOS job. Unselected jobs are skipped. The source gate,
`Source and product contracts`, accepts a skip only for a scope the classifier
deselected, a macOS job on a pull request, or the opt-in Linux native job, and
fails on any other result,
including `cancelled` and `abandoned`. A newer pull-request or `main` push cancels
the older run. A `main` run classifies every change since the last successful
`main` push run, so a cancelled run's changes stay in scope, and a burst of merges
runs the macOS jobs once. Manual `workflow_dispatch` and release validation always run
all source and macOS checks; only the explicit dispatch option adds Linux GUI validation. When GitHub cannot assign a macOS runner, the job ends cancelled and
the `main` run fails; rerun it with `gh run rerun <run-id> --failed`.
Actions are pinned to commit SHAs. Toolchain pins:
Node 26.13.0 LTS (root `.nvmrc`), Rust 1.97.1, uv 0.11.2,
Syft 1.50.0. Node setup in CI and release validation/signature verification reads
the same pin. npm caching is explicit and disabled in signing/publication jobs.

Source jobs use the same bounded check runner as local development, with a
60-minute job limit and per-command timeouts. Native and package jobs also have
a 60-minute job limit. Their receipts and logs upload on
success or failure and expire after seven days. The web gate includes agent and
World evaluator regression tests; Rust also checks the `eval-harness` contracts.
The web job runs as two parallel legs that split the Vitest files with
`check.sh web --shard 1/2` and `--shard 2/2`. The first leg also runs the Node, source and harness checks; the second runs
layout and renderer build checks after its renderer-test shard. A contract test
compares both legs with the complete check plan, allowing duplication only for
setup and the sharded renderer step. Raising
Vitest workers does not help on the 4-vCPU runner: with the same 1593 tests, four
workers took 482 s and two took 464 s.

Rust runs three parallel legs on each selected host: `lint` (formatting and both
Clippy builds), `tests` (all-target tests) and `eval` (headless eval contracts and
the Python sandbox tests). The Python worker admits one run at a time, so its
tests form a serial chain of cold module compiles and deliberate timeouts that
outlasts every other test (about 135 s of a 176 s local run). `check_runner.py`
skips that chain in `tests` and runs it as `python-sandbox-tests` in the `eval`
leg's existing build, with one filter list for both sides, so each test runs once
and the two legs finish in similar time.
Lint installs only Rust; the other legs install locked Node and OpenBB dependencies
for host integration tests. The plans still execute every Rust step exactly once
on each host. `check.sh rust --only <steps>` runs the same parts locally. Rust
caches include the Python worker workspace when it exists, so its Wasmtime
dependencies are not rebuilt from scratch by each runtime consumer. Ubuntu installs
Tauri's GTK/WebKitGTK development libraries; the [Linux development
procedure](dev/checks.md#linux-cloud-development) uses the same source gate.
These gates make no paid model calls. See [broader checks](dev/checks.md#broader-checks).

For cloud-native validation, run
`gh workflow run ci.yml --ref <branch> -f linux_native=true`.
`Native Linux interaction (WebKitGTK)` then runs `test:native:full` and
`test:persistence` with no `DISPLAY`, so the GUI lease starts a private Xvfb.
The [same commands](../apps/loupist/e2e/README.md#run-native-suites-on-linux) remain
available to Codex, Claude and Cursor cloud agents while implementing UI, bridge
or storage changes. Linux does not qualify the macOS release surface.

Unsigned macOS native/package jobs reuse an exact-source runtime cache through
`.github/actions/stage-ci-runtimes`. The key covers staging scripts, runtime source
and pins, embedding assets and toolchain/host versions. Only successful main-push
staging can save it, and there are no fallback keys. Before running a restored
binary, a complete file/mode/digest inventory rejects missing, added, changed or
linked files. The current package-prerequisite checks run on every hit; a missing
or invalid cache rebuilds from pinned sources. Caches contain only staged resources,
never `.venv`, credentials or user data. Signed release jobs do not use this cache:
they stage afresh with their signing identity after full exact-commit CI.
[GitHub cache access rules](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)
and the [Rust cache key contract](https://github.com/Swatinem/rust-cache#example-usage)
explain the trusted-main writes and cache selection. Tests and eval use the same
dependency cache because `eval-harness` adds no dependencies; workspace crate
outputs are not retained by the Rust cache. Lint uses its own cache.

The macOS native job stages the same pinned sidecars used by the package smoke
and first selects a supported CI display mode of at least 1920x1080. The helper
only runs in GitHub Actions and changes the runner's login session; it fails if
the visible desktop cannot fit the required 1680x820 test window. Test geometry
and assertions remain the same as local runs. The job drives an isolated native
WebView through onboarding, Loupist management,
Connections, Knowledge, Chat lifecycle and accessibility. Its persistence suite
creates a Loupist, a Chat, Library items (Source, principle, Journal entry) and
settings, then checks them after a restart and after opening a copy of the Loupist
folder in a fresh app. It uses
no provider or connector credentials; the live release journeys
(`npm run test:live-journeys` in `apps/loupist/e2e/`) are a pre-release
acceptance step rather than CI input.

A release tag is accepted only from a `main` commit whose scoped `ci.yml` push
run succeeded. Before signing, the tag workflow calls the same-commit `ci.yml`
with `full: true`: all source, native and unsigned package checks must pass.
This reusable workflow receives no signing or provider secrets. A green
documentation-only push is therefore insufficient to sign a release.
Dry runs skip the tag-only source gates and repeat full CI only on request;
[rehearsal and signing modes](release.md#signing-modes-and-rehearsal) owns that path.
Assembly has a read-only token and no environment: it verifies updater signatures
against the embedded public key in the separate signing report. Publication is
skipped in dry runs; otherwise a second `release` approval gates the write token,
verification against the configured updater public key, attestations and release
creation. The signing report is an artifact because GitHub can suppress secret-like
job outputs (including the Developer ID team).
Release validation, assembly/promotion and signing jobs have 15, 30 and 90 minute limits
respectively; signed package preflight has a 60 minute limit.

Official installers and automatic-update manifests include only macOS Apple
Silicon. Asset and qualification verifiers require this exact platform set.

The only required check is `Source and product contracts`, owned by GitHub Actions,
with no bypass. Pull requests never report `Native macOS interaction`,
`Package smoke (aarch64-apple-darwin)`, `Rust lint/tests/eval (macOS)`,
`Native Linux interaction (WebKitGTK)`, so no `main` ruleset or legacy branch protection may require them. The
read-only release audit flags them.

### Timing baseline (2026-11-09)

The five latest successful PR runs took 6m43s–9m58s (median 7m38s).
[PR #54's final run](https://github.com/loupistai/loupist/actions/runs/42555521674)
spent 193s on ordinary Rust tests and another 107s on eval contracts in one leg;
its first web shard also carried 75s of layout/build work. Linux native UI used
6m16s–6m56s per sampled run but was not the critical path. Moving it to an explicit
cloud check saves runner time; it does not by itself explain a wall-time reduction.

The [latest green main run](https://github.com/loupistai/loupist/actions/runs/42556292654)
took 19m35s: macOS Rust took 19m14s and packaging 18m14s. The Rust receipt included
236s of Clippy, 404s of tests and 333s of evals in sequence. Mac native/package
staging separately took 226s and 309s. Two earlier green main runs took 21m38s
and 25m33s. These timings motivate parallel Rust legs, balanced web work and
verified staging reuse; they are a baseline, not measurements of the new workflow.
Cache misses and runner queues still affect elapsed time. Record the new exact-head
CI timings before claiming a speedup. There were no release-workflow runs available
for a measured CD baseline; signing and publication gates remain intact.

### Model-provider authentication and live acceptance

Ordinary and full CI use scripted Pi responses, connection replays and isolated
native fixtures. They check runtime contracts, tool execution, permissions,
Memory persistence and evaluator behavior; they do not measure real model
answer quality, tool selection or live sign-in. No model-provider secrets are
required in GitHub Actions.

Real-model checks remain explicit owner-run commands in an authenticated local
environment: the [headless agent runner](../apps/loupist/evals/README.md#headless-agent-runner)
for prompt/Skill/Memory changes, and the
[live release journeys](../apps/loupist/e2e/README.md#explicit-live-suites) for
app acceptance. They consume model usage. Pass the existing signed-in Pi
directory in place using `--pi-agent-dir` or `LOUPIST_LIVE_PI_AGENT_DATA_DIR`;
do not copy credential files into the checkout, Actions secrets or artifacts.
An authentication failure is a failed live check, not permission to silently
substitute a mock or another model. Record the tested revision, model, suite and
result separately from credential-free CI. Stable qualification continues to
require the signed candidate's Chat/Memory acceptance confirmation and evidence.

## Local bundle inspection

After creating a macOS bundle, run:

```sh
sh apps/loupist/scripts/check-macos-app-bundle.sh "/path/to/Loupist.app"
```

`native-e2e-linux` stages that runtime only when the native job is selected.
`verify-rust` still runs the offline runtime check on Linux when native is skipped.

The script checks Pi/OpenBB, the pinned Python runtime and embedding inventory,
embedding bytes, safe links, architecture, launchers and signatures, then runs
the offline Pi/OpenBB sidecar checks and the signed Python worker. The worker
must carry the hardened runtime and its executable-memory entitlement. The
Loupist app binary must not. The script does not download missing resources.
Runtime staging is documented in
[Development](dev/checks.md#runtime-staging-and-package-checks).
