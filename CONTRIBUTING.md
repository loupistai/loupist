# Contributing to Loupist

Loupist is licensed under AGPL-3.0-only. See [Licensing](LICENSING.md) for app code, contributions, shared packs, provider data and Gallery terms. By contributing, you agree to the [Contributor License Agreement](CLA.md) and certify that you have the right to submit the work.

## Development

- Find the shared instructions in [AGENTS.md](AGENTS.md) and the [topic guide](docs/README.md).
- Start with [desktop development](apps/loupist/README.md) and use the
  [native UI guide](apps/loupist/e2e/README.md) for app testing.
- Preserve the [React/Rust/sidecar authority boundaries](README.md#security-model).
- Prefer deleting obsolete pre-1.0 paths over compatibility shims.
- Prove behavior with tests. Avoid wording and implementation locks; reserve source scans for release pins and security invariants.
- Update docs when the product story changes.
- Run `scripts/verify.sh` before requesting review.
- Never include credentials, databases, user memory, staged executables, or generated build output.

## Branches and pull requests

Use `main` as the integration and release base. Create a short-lived branch for
each change; coding agents use `codex/<topic>`. Open a pull request targeting
`main`, describe the observable change and list the checks that established it.
Use `git commit -s` to add the required Developer Certificate of Origin sign-off.
A GitHub-provided noreply address can keep your personal email private.

Merging requires the branch to be current with `main`, the required CI checks to
pass and review conversations to be resolved. Squash merge preserves a linear
history; GitHub deletes the merged branch. Direct pushes, force pushes and branch
deletion are blocked on `main`, including for administrators. While the project has
one maintainer, another person's approval is not a merge prerequisite; maintainers
still review changes and the CODEOWNERS file routes review requests. Stable release
publication has a separate independent-approval gate.

Dependabot opens grouped weekly updates. Review dependency, license and runtime-pin
changes and let the same CI run before merging; dependency updates are not
automatically merged.

See [Security policy](SECURITY.md) for private vulnerability reporting.
