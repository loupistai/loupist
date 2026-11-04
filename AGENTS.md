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

Run the narrowest observable behavior or security test. Reserve source scans for
security invariants and release pins. For native work read
[Which tool](apps/loupist/e2e/README.md#which-tool). `scripts/verify.sh` is the
handoff gate. Keep credentials private and generated schemas/build output untouched.
Prefer deleting obsolete pre-1.0 paths. Commit, push or release only on request.

For cross-session work, leave a brief [task handoff](docs/dev/repository-guidance.md#task-handoff).

Linux, including Cloud Agents, does not run the macOS window. Use the design preview and the Linux checks in [Setup across development hosts](docs/dev/checks.md#setup-across-development-hosts).
