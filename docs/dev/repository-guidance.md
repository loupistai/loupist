# Repository guidance

## Public repository

The product name is Loupist; repository paths, bundle identifiers and local storage
names use `loupist`. The configured GitHub repository is `loupistai/loupist`.

## Owners and updates

| Information | Owner | When it changes |
| --- | --- | --- |
| Rules that apply in every coding session | [AGENTS.md](../../AGENTS.md) | Change the root only when every task needs the constraint. |
| A product or engineering decision worth keeping | The affected section under [docs](../README.md) | In the behavior change, record the choice, why the alternative failed and the command or code that can disprove it. Replace superseded guidance. Git preserves the prior rationale. |
| Repeatable developer procedure | The script/test that performs it and its owning guide section | Keep the procedure beside its executable owner. |
| Current task state | Its author and receiver, in the PR or handoff message | On switching sessions or transferring unfinished work. Keep durable decisions in the owning guide section. |

Search with `python3 scripts/guide.py <topic>`. It returns up to twelve section
addresses from Git-visible guides, including new uncommitted pages. When a title
misses, search the body with `rg`. Run `python3 scripts/guide.py --check` after
changing routes. Every gate and CI run checks it too. Inspect Git-visible new files with
`git ls-files -o --exclude-standard` and review their contents before commit. Keep private notes under `docs/local/`, plans in `plan/` and run artifacts in
`output/`. Setup in a clean checkout uses the guidance at that revision.

## Tool entry points

| Coding environment | Entry |
| --- | --- |
| Codex (GPT), Cursor, Grok Build | Open the trusted repository at its root. Use `AGENTS.md`. |
| Claude Code | `CLAUDE.md` imports the same root. |
| Orca | Open the checkout with the selected coding agent. The underlying agent reads its own project entry. |
| Cursor, Claude Code and Codex cloud agents | The same entry files, plus `scripts/cloud-setup.sh` ([cloud setup](checks.md#setup-across-development-hosts)). |

Check only reviewed entry points into Git: `.cursor/worktrees.json`,
`.cursor/environment.json`, `.codex/environments/environment.toml` and the
cloud-only setup hook in `.claude/settings.json`. Tool-local permissions, chat
histories, other hooks and personal skills belong to their developer.

## Code cleanup

Trace callers from the current app, preview, native host, scripts, and tests before
deleting a path. A component imported only by its dedicated test can still be
obsolete. Move any useful behavior coverage to the current component, then remove
the old implementation and its test. Keep host commands that agent tools or native
callers still use, even when React no longer calls them.

Run `npm --prefix apps/loupist run check:unused` to find renderer files unreachable
from the maintained entry points. The pinned Knip configuration in
`apps/loupist/knip.jsonc` includes the app, browser preview, feedback exporter, and
native E2E callers. Tests are excluded so they cannot keep retired UI code alive.
Declaration files are checked by TypeScript. Add an entry only for a real external
caller, and name that caller in the change. Do not add a file as an entry merely
to silence the check. See [Knip project files](https://knip.dev/guides/configuring-project-files).

This check covers renderer file reachability. It does not establish that every
export, Rust command, Python function, or dynamic import is needed. Inspect callers
before removing those. Use `scripts/guide.py` to update the owning section and run
the narrow behavior tests before the full handoff gate.

Keep repeatable checks in the existing check runner. Add a new instruction only
when a test, type, or existing check cannot enforce the rule. Prefer deleting a
retired path over preserving a second implementation for a hypothetical caller.

## Task handoff

When transferring unfinished work, leave a brief PR description or handoff message
with the goal, branch and revision, remaining work, and the last verification result.
Identify where uncommitted edits live and who owns them; a message does not transfer
those edits. The receiver checks the current status and diff, reruns affected checks
if the tree changed, and confirms window ownership before native interaction.
Keep credentials, raw transcripts, screenshots and customer research out of the
message. Fold durable decisions into the owning guide section when work completes.

## Product Memory and Skills

Loupist stores its users' research in their local workspaces. Product procedures
ship from `apps/loupist/agent/skills/`. The Rust workspace snapshot admits selected
instructions and plain skill directories from its configured roots and skips
symlinks. Developers keep handoffs outside those roots. If someone deliberately
opens this repository as a Loupist research workspace, its root `AGENTS.md` is an
eligible workspace instruction.
See the [product context contract](../runtime/chat-turns.md#local-workspace-execution).

Revisit these entry points if a supported client stops discovering the shared source
or a UI surface cannot be observed with the current tools.
