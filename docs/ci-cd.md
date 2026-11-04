# CI and releases

First-release target: macOS 13+ on Apple Silicon only. Windows implementation is retained for future support; Intel Macs and Linux are also outside the release scope. Repository identity is `loupistai/loupist`.

This page is the release procedure. Product behavior is not defined here. Workflows and scripts in the repo are the source of truth for smoke details.

## GitHub

The official repository is `loupistai/loupist` under the Loupist AI organization.
Organization members require two-factor authentication. Base repository permissions
are `none`; grant access explicitly per repository or team. Organization owners
control repository visibility changes, deletion, transfer and GitHub App installation.

`main` is the integration and release base. Work arrives through short-lived
branches and pull requests; coding agents use `codex/<topic>`. Use squash merges,
delete merged branches, require an up-to-date branch and resolved conversations,
and apply the same `main` rules to administrators. The initial single-maintainer
setup requires CI and a PR with zero mandatory approving reviews. CODEOWNERS routes
reviews; add a review-count requirement when another maintainer can provide it.

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

PRs and `main` pushes select checks by changed paths. Every run checks guide
links, scope regression tests, credential boundaries and Node pins without
installing npm, OpenBB or Rust dependencies. The selected source checks run on
Ubuntu (web, Python) and both macOS and Ubuntu (the same Rust gate), alongside
native and package jobs. The Linux Rust checks do not need a display.

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

Repeat compiles restore Rust caches populated
only by successful jobs on `main` (push or manual); PRs restore them without
uploading redundant branch caches.
The signed build shares the package-smoke dependency cache. Official package CI
has one explicit macOS job; experimental Windows packaging stays manual-only.
Unselected source jobs are skipped; the source gate accepts a skip only when
the classifier explicitly deselected that scope. Native and package jobs retain
their required check names using inexpensive Ubuntu no-op steps when unselected.
Manual `workflow_dispatch` and release validation always run the full set.
Actions are pinned to commit SHAs. Toolchain pins:
Node 26.13.0 LTS (root `.nvmrc`), Rust 1.97.1, uv 0.11.2,
Syft 1.50.0. Node setup in CI and release validation/signature verification reads
the same pin. npm caching is explicit and disabled in signing/publication jobs.

Source jobs use the same bounded check runner as local development, with a
60-minute job limit and per-command timeouts. Native and package jobs also have
a 60-minute job limit. Their receipts and logs upload on
success or failure and expire after seven days. The web gate includes agent and
World evaluator regression tests; Rust also checks the `eval-harness` contracts.
Each Rust job installs the locked Node dependencies and OpenBB Python environment:
host integration tests exercise Pi prompts and Python process ownership, not just
Rust code in isolation. Ubuntu also installs Tauri's GTK/WebKitGTK development
libraries; the [Linux development procedure](dev/checks.md#linux-cloud-development)
uses the same source gate. When Rust is selected, the aggregate source check
requires both Rust jobs.
These gates make no paid model calls. See [broader checks](development.md#broader-checks).

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

`Native Windows interaction` and `Package smoke (x86_64-pc-windows-msvc)` must
not be required by any applicable `main` ruleset or legacy branch protection.
Required checks are `Source and product contracts`, `Native macOS interaction`,
and `Package smoke (aarch64-apple-darwin)`, with strict GitHub Actions ownership and
no-bypass protections. The read-only release audit flags obsolete Windows gates.

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

## Release

SemVer tags and the version helper. [Releases](release.md#release).

## Candidate update qualification

The manual pass before an update is published. [Releases](release.md#candidate-update-qualification).


## Local bundle inspection

After creating a macOS bundle, run:

```sh
sh apps/loupist/scripts/check-macos-app-bundle.sh "/path/to/Loupist.app"
```

The script checks Pi/OpenBB and embedding inventory, embedding bytes, safe links,
architecture/deployment targets, launchers and signatures, then runs the offline
Pi/OpenBB sidecar smoke checks. It does not download missing resources. Runtime staging is documented in
[Development](dev/checks.md#runtime-staging-and-package-checks).
