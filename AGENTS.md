# Developing Loupist

Loupist is a local-first financial research desktop app. It never trades.

Start with `git status --short`. Preserve other sessions' edits and window ownership.
Use `python3 scripts/guide.py <topic>` to find a section, then pass its
`path#heading` back to read it. [Browse by area](docs/README.md).
Update the owning section when behavior or a durable decision changes.
When replacing a path, remove its obsolete implementation and dedicated tests.
Preserve behavior coverage through the current entry point. Use the
[cleanup checks](docs/dev/repository-guidance.md#code-cleanup) before deleting code.

React submits typed intent via `apps/loupist/src/bridge/tauriBridge.ts`. Rust owns
credentials, persistence, Memory transactions, grants and worker lifetime.
Pi's native file tools stay inside the admitted workspace, scratch, attachment and
global-instruction paths ([path classes](docs/runtime/permissions.md#path-classes)). `run_python` is the only agent code execution tool. It runs in the pinned
WASI worker, with no shell, network, DNS, process spawn or host environment.
The Loupist app binary does not receive the worker's unsigned-executable-memory
entitlement. Keep Pi's runtime CWD private, direct file tools to the trusted
workspace, and load only the host's bounded instructions and Skills. File changes
survive cancellation. Python may write only `outputs/`, this run's scratch and
existing non-dot top-level workspace subdirectories the host grants. Never enable
automatic project settings or JS extensions to obtain workspace support.

## Tests

Keep shared code and its tests working on macOS and Linux
([host setup](docs/dev/checks.md#linux-cloud-development)). On Linux, drive the real
app with the native suites, `up.sh` and `agent-driver.mjs`; they start Xvfb when no
display is set ([Linux runs](apps/loupist/e2e/README.md#run-native-suites-on-linux)).
macOS Apple Silicon is the release target: verify WKWebView, Keychain, OS input and
release acceptance on macOS. A Linux or browser-preview pass does not establish those.

Run the narrowest observable behavior or security test. Reserve source scans for
security invariants and release pins. For native work read
[Which tool](apps/loupist/e2e/README.md#which-tool). Keep credentials private and
generated schemas/build output untouched. Prefer deleting obsolete pre-1.0 paths.

## Pull requests

Before handoff, run `scripts/verify.sh --affected` on the final source tree. It
runs the repository checks and the web, python and rust scopes that CI's path
classifier selects for the changes since `origin/main`; an unclassified path
selects every scope ([gate](docs/dev/checks.md#broader-checks)). Linux cloud agents
(Codex, Claude or Cursor) start from the pinned
[cloud setup](docs/dev/checks.md#setup-across-development-hosts). On either
host, run `npm --prefix apps/loupist/e2e run test:native` when UI flow, bridge or
persistence changes; add persistence coverage for storage/restart changes.
PR CI checks shared code on Linux. Native UI is checked on macOS after merge;
Linux GUI CI is an explicit cloud-validation dispatch, so it does not substitute
for an agent's affected native check ([CI](docs/ci-cd.md#ci)). Record the host and
unrun platform boundaries. Commit, push or release only on request; a task that
asks for a PR covers pushing its own branch.
Name the branch `<tool>/<topic>`, sign off with `git commit -s`, target `main`, and
list the checks you ran. Never push to `main` or create release tags.

For cross-session work, leave a brief [task handoff](docs/dev/repository-guidance.md#task-handoff).
