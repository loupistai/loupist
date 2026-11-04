# CI and releases

First-release target: macOS 13+ on Apple Silicon only. Windows implementation is retained for future support; Intel Macs and Linux are also outside the release scope. Repository identity is `loupistai/loupist`.

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
npm, Cargo and uv; review them through the ordinary CI and PR process. Private
vulnerability reporting, Dependabot alerts/updates, secret scanning and push
protection belong in the repository settings.

OpenBB and its extensions receive grouped minor/patch updates. Major upgrades are
coordinated runtime changes: migrate the base package, extensions, manifest and
sidecar contracts together rather than proposing incompatible individual majors.

CodeQL default setup analyzes Actions, JavaScript/TypeScript and Python on GitHub.
Rust is checked by the Rust CI job's formatting, Clippy and behavior/security tests.

Release, qualification and promotion workflows are initially disabled in GitHub.
Keep them disabled until a maintainer explicitly authorizes the first release and
completes signing setup and the release audit. CI package smokes publish only
short-lived workflow evidence, not GitHub Releases or update manifests.

Require pull requests for `main`. Restrict `v*` tag creation, update, and deletion. Protect the `release`, `release-qualification`, and `stable-release` environments. If an environment uses selected deployment branches and tags, allow the `v*` **tag** pattern for each tag-triggered release job. Signing secrets live only in `release`. `stable-release` must require at least one reviewer, prevent self-review, and disallow administrator bypass, so the person who dispatches promotion cannot approve or force the publication. Enable immutable GitHub Releases.

Before every RC tag and stable promotion, run the checked-in, read-only audit
from an authenticated GitHub CLI session:

```sh
python3 apps/loupist/scripts/check-github-release-setup.py
```

It verifies the public repository identity, immutable releases, `main`
pull-request protection, an active `v*` tag rule that restricts creation,
updates, and deletion, each required protected environment, and the **names** of every
secret referenced by `release.yml`. Those names must be present only in the
`release` environment, never at repository scope or in either non-signing
environment. It never prints or reads secret values and does not change GitHub
state. A nonzero result names every missing item. The auditor intentionally
derives the required release-secret names from the workflow so a future signing
change cannot silently make the checklist stale.

The immutable-release endpoint requires repository administration read access.
Release workflows intentionally do not receive an administrative token, so the
maintainer's authenticated, read-only audit is the release gate for that setting
rather than adding a broader long-lived credential to every release job.

## CI

Pull requests run only Linux jobs: web, Python, and Rust lint and tests, selected
by changed paths. They never wait for a macOS runner. The `main` push run, manual
`workflow_dispatch` and the release call also run the macOS Rust contracts, native
interaction and package smoke for the selected scopes. To run the macOS jobs on a
branch before merging, use `gh workflow run ci.yml --ref <branch>`; a dispatch runs
the full set. Every run checks guide links, scope regression tests, credential
boundaries and Node pins without installing npm, OpenBB or Rust dependencies. The
Linux Rust checks do not need a display.

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
| App dependencies, toolchain/CI configuration, staging scripts or unknown paths | All |

Mixed changes take the union of their scopes. Shared renderer contract fixtures
also select Rust, and agent eval fixtures select Web and Rust. Inline Rust tests
in production files retain native/package checks. PRs use a merge-base diff;
pushes use the complete before/after range. Deleted and renamed source paths
remain in scope. Missing history or classification errors select all checks.
Native interaction and packaging in the table run only outside pull requests.

Rust and uv caches are saved only by successful `main` runs; pull requests restore
them without uploading branch copies. The signed build shares the package-smoke
dependency cache. Official package CI has one explicit macOS job; experimental
Windows packaging stays manual-only. Unselected jobs are skipped. The source gate,
`Source and product contracts`, accepts a skip only for a scope the classifier
deselected or a macOS job on a pull request, and fails on any other result,
including `cancelled` and `abandoned`. Pull requests cancel their superseded runs.
Every `main` push keeps its run, because the next push classifies only its own
before/after range. Manual `workflow_dispatch` and release validation always run
the full set. When GitHub cannot assign a macOS runner, the job ends cancelled and
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
`check.sh web --shard 1/2` and `--shard 2/2`. The first leg also runs every other
web step; the second runs only the installs and its renderer-test shard. Raising
Vitest workers does not help on the 4-vCPU runner: with the same 1593 tests, four
workers took 482 s and two took 464 s.

Linux Rust runs as two parallel jobs that split the Rust plan. `lint` runs
formatting and both Clippy builds and installs only Rust. `tests` runs the Rust
tests and `eval-harness` contracts and installs the locked Node dependencies and
OpenBB Python environment, because host integration tests exercise Pi prompts and
Python process ownership. `check.sh rust --only <steps>` runs one part locally, and
a scope test keeps the two parts equal to the whole plan. The macOS Rust job runs
the whole plan. Ubuntu also installs Tauri's GTK/WebKitGTK development libraries;
the [Linux development procedure](dev/checks.md#linux-cloud-development) uses the
same source gate. These gates make no paid model calls. See
[broader checks](development.md#broader-checks).

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
Release validation, assembly/promotion and signing jobs have 15, 30 and 90 minute limits
respectively; signed package preflight has a 60 minute limit.

Official installers and automatic-update manifests include only macOS Apple
Silicon. Asset and qualification verifiers require this exact platform set.

`windows-compatibility.yml` is manual-only and independent of `ci.yml`. It keeps
Windows native interaction, persistence, and unsigned installer checks available.
Its `signed` option, dispatched from the matching version tag, runs the retained
Authenticode build and uploads workflow artifacts; it requires Windows secrets in the protected `release`
environment and never publishes release assets or updates. Those optional
secrets are not prerequisites for the macOS release audit.

The only required check is `Source and product contracts`, owned by GitHub Actions,
with no bypass. Pull requests never report `Native macOS interaction`,
`Package smoke (aarch64-apple-darwin)`, `Rust contracts (macOS)` or the Windows
checks, so no `main` ruleset or legacy branch protection may require them. The
read-only release audit flags them.

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

The script checks Pi/OpenBB and embedding inventory, embedding bytes, safe links,
architecture/deployment targets, launchers and signatures, then runs the offline
Pi/OpenBB sidecar smoke checks. It does not download missing resources. Runtime staging is documented in
[Development](dev/checks.md#runtime-staging-and-package-checks).
