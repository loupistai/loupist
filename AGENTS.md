# Developing Loupist

Loupist is a local-first financial research desktop app. It never trades.

Start with `git status --short`. Preserve other sessions' edits and window ownership.
Use `python3 scripts/guide.py <topic>` to find a section, then pass its
`path#heading` back to read it. [Browse by area](docs/README.md).
Update the owning section when behavior or a durable decision changes.

React submits typed intent via `apps/loupist/src/bridge/tauriBridge.ts`. Rust owns
credentials, persistence, Memory transactions, grants and worker lifetime.
Pi's native file and Bash tools run with host permissions. Keep its runtime CWD
private, direct tools to the trusted workspace, and load only the host's bounded
instructions and Skills. File changes survive cancellation. Never enable automatic
project settings or JS extensions to obtain workspace support.

## Tests

Develop and run source checks on both macOS and Linux using the
[host setup and check commands](docs/dev/checks.md#linux-cloud-development).
Keep shared code and its tests working on both hosts. macOS Apple Silicon remains
the release target; native WKWebView, Keychain, OS input and release acceptance
must be verified on macOS. A Linux source or browser-preview pass does not
establish those native results.

Run the narrowest observable behavior or security test. Reserve source scans for
security invariants and release pins. For native work read
[Which tool](apps/loupist/e2e/README.md#which-tool). Chrome design preview does not
prove WKWebView layout: a `fieldset` with `display: grid` or `flex` collapses its
legend to one character there and looks fine in Chrome. Measure those headings with
`assertFormLegendsFit` in the macOS WebView
([UI review](docs/dev/ui-qa.md#ui-review)). `scripts/verify.sh` is the
handoff gate. Keep credentials private and generated schemas/build output untouched.
Prefer deleting obsolete pre-1.0 paths. Commit, push or release only on request.

For cross-session work, leave a brief [task handoff](docs/dev/repository-guidance.md#task-handoff).
