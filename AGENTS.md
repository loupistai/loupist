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
Pi's native file and Bash tools run with host permissions. Keep its runtime CWD
private, direct tools to the trusted workspace, and load only the host's bounded
instructions and Skills. File changes survive cancellation. Never enable automatic
project settings or JS extensions to obtain workspace support.

## Tests

Keep shared code and its tests working on macOS and Linux
([host setup](docs/dev/checks.md#linux-cloud-development)). macOS Apple Silicon is
the release target: verify WKWebView, Keychain, OS input and release acceptance on
macOS. A Linux or browser-preview pass does not establish those native results.

Run the narrowest observable behavior or security test. Reserve source scans for
security invariants and release pins. For native work read
[Which tool](apps/loupist/e2e/README.md#which-tool). Keep credentials private and
generated schemas/build output untouched. Prefer deleting obsolete pre-1.0 paths.

## Pull requests

Locally, `scripts/verify.sh` is the handoff gate. In a cloud session, run
`sh apps/loupist/scripts/check.sh <web|python|rust>` for what you changed; PR CI
runs the Linux gate and macOS checks run after merge ([CI](docs/ci-cd.md#ci)). Commit, push
or release only on request; a task that asks for a PR covers pushing its own branch.
Name the branch `<tool>/<topic>`, sign off with `git commit -s`, target `main`, and
list the checks you ran. Never push to `main` or create release tags.

For cross-session work, leave a brief [task handoff](docs/dev/repository-guidance.md#task-handoff).
