# Verification and remaining native check

Product source revision is `9cc7748b1f20db482de6fc0f0c9d64d4b8bd62fe`.
Later changes in this handoff contain only audit documents. No merge or release
is part of this task.

## macOS source gate

`scripts/verify.sh` passed all 35 steps on Apple Silicon with Node 26.13.0,
Rust 1.97.1 and the locked Python worker environment. The run used
`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=1` and
`CARGO_PROFILE_TEST_DEBUG=1`.

| Check | Observed result |
| --- | --- |
| Web gate | 20 steps passed, including dependency installation |
| Node contracts | 446 passed, 1 skipped |
| Renderer | 1,585 passed across 228 files |
| Chat layout and renderer build | Passed |
| Python gate | 10 steps passed, including 57 script tests and 50 worker tests |
| Rust gate | Format, normal Clippy, E2E Clippy, all-target tests and headless eval contracts passed |
| Rust all-target tests | 1,155 library tests and 2 integration tests passed, 3 live tests ignored |
| Headless eval contracts | 56 passed, 3 live tests ignored |

The official receipt is
`apps/loupist/output/checks/20261105T015254Z-b775d861/receipt.json`.
It records the same sealed tree before and after the run,
`0240a48e69c785fd5d63bd0354631dd4f634ad29`, and
`source_changed_during_run: false`. The tree includes the then-untracked audit
documents; tracked product files were clean. The run took 639.132 seconds.
The complete console log is `/tmp/loupist-research-macos-verify.log`.

## Linux source gates

An independent verifier ran a clean archive of the same product commit in an
isolated Ubuntu 24.04 ARM64 container with Node 26.13.0, Rust 1.97.1,
Python 3.12.3 and uv 0.9.6. Its tracked source tree remained
`40e155d188b09939588ab4699a10e76b22b2f87a` throughout both gates.

| Check | Observed result |
| --- | --- |
| Web gate | 18 steps passed |
| Node contracts | 445 passed, 2 skipped |
| Renderer | 1,585 passed across 228 files |
| Chat layout and renderer build | Passed |
| Focused learning suite | 24 passed |
| Rust gate | All 5 steps passed |
| Rust all-target tests | 1,149 passed, 3 live tests ignored |
| Headless eval contracts | 46 passed, 3 live tests ignored |

The verifier used Knip's documented `KNIP_DISABLE_RAW_TRANSFER=1` parser
fallback. GNU ld exceeded the container's memory limit; a task-local compiler
wrapper selected Rust's bundled LLD 22.1.6, with one build job. The source,
test assertions and check commands were unchanged. Failed environment attempts
remain in the logs. After verification, the verifier removed only its private
build target and returned its freed blocks through the authorized filesystem
trim. It did not change the VM profile or another session's files.

The report is `/tmp/loupist-research-linux/verification-report.json`.
Official receipts and their adjacent logs are:

- `/tmp/loupist-research-linux/checks-9cc7748/20261105T012816Z-5dd5d311/receipt.json`.
- `/tmp/loupist-research-linux/checks-9cc7748/20261105T014710Z-cbcfb049/receipt.json`.

## Native UI boundary

`LOUPIST_E2E_ALLOW_UNFOCUSED=1 npm --prefix apps/loupist/e2e run test:native`
was attempted after the source gate. It exited before launching an app because
another session's existing `manual-preview` window owns the per-user native GUI
lease. The launcher reported that no app was launched or stopped. The log is
`/tmp/loupist-research-native.log`.

The existing window and its ownership were preserved. No lease bypass, profile
reset or process termination was used. Native smoke, changed Lens/receipt
WKWebView layout, native keyboard behavior and form-legend measurements are
**not verified** by this task. Chrome previews and source tests do not establish
those results.

The next native owner should arrange a window handoff, run the isolated native
smoke, then inspect the changed Lens and receipt fixtures in both locales and
themes at minimum and normal window widths. Follow
`apps/loupist/e2e/README.md#which-tool`; use `assertFormLegendsFit` for the forms.
The branch is `codex/autonomous-research-harness`; no product edits remain
uncommitted. Check the current source revision before rerunning.

## Research demonstration and review

The paid production-path development demonstration and its failed first attempt
are documented in [evaluation.md](evaluation.md). Two retained investigations
created one candidate through the production learning worker. A fresh relevant
question read and applied that exact method; the unrelated calculation did not.
Read-only probes committed no Memory. This verifies retention and scoped reuse.

The Source-only comparison also answered the main question correctly. The
learned-method answer had one malformed citation target, used more total input
and took longer in this single sample. No research superiority or investment
alpha is established. The frozen 40-case outcome comparison was not run.

Independent review and the fixed exclusion-control race are documented in
[reviews.md](reviews.md). The design and rejected alternative are in
[design.md](design.md); [decisions.tsv](decisions.tsv) records the decision trail.

Knowledge schema 6 intentionally refuses earlier folders without migration or
deletion. Existing folders remain intact and need a compatible build. This is a
material compatibility boundary, not a successful migration result.
