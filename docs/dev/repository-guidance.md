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
changing routes. The web gate runs it too. Inspect Git-visible new files with
`git ls-files -o --exclude-standard` and review their contents before commit. Keep private notes under `docs/local/`, plans in `plan/` and run artifacts in
`output/`. Setup in a clean checkout uses the guidance at that revision.

## Tool entry points

| Coding environment | Entry |
| --- | --- |
| Codex (GPT), Cursor, Grok Build | Open the trusted repository at its root. Use `AGENTS.md`. |
| Claude Code | `CLAUDE.md` imports the same root. |
| Orca | Open the checkout with the selected coding agent. The underlying agent reads its own project entry. |

Check only reviewed entry points into Git. Tool-local permissions, chat histories,
hooks and personal skills belong to their developer.

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
