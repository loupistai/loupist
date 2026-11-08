# Native tool permissions

What a native Pi tool (`read`, `ls`, `find`, `grep`, `write`, `edit`, `bash`) may do in one Chat reply, decided by Rust from the target's path class and how much third-party text the model has seen. Ordinary work never shows a card; one card appears where a person's judgment adds safety.

The policy is one pure module,
`apps/loupist/src-tauri/src/agent_harness/native_policy.rs`. The extension
(`apps/loupist/agent/workspace-tools.mjs`) asks `native.authorize` before every
native execution with the resolved file (`read`, `write`, `edit`), the resolved search
root (`ls`, `find`, `grep`; the workspace by default) or the exact command (`bash`).
`AppToolExecutor::native_authorize` (`commands/tool_executor/workspace.rs`) parses the
request once, decides, and asks the user through an approval card when the policy
says so. Unattended runs refuse every native tool before this policy
(`briefings/policy.rs`).

## Exposure

| Exposure | When |
| --- | --- |
| Clean | No third-party text this turn, and no earlier reply in this Chat read any. |
| ThisTurn | This turn raised the untrusted-text tier ([Untrusted text](untrusted-text.md)). |
| Carried | This turn is clean, but an earlier reply in this Chat recorded `tool_trace.untrusted_text`. The window is the whole Chat: neither the warm Pi session nor the cold transcript records which replies it holds. Carried affects only this policy. |
| Custom | This turn read a custom-connection result. File rows behave as ThisTurn; Bash stays refused by `TurnTaint::enforce` (`outbound_paused`). |

## Path classes

Targets are canonicalized first (symlinks of the existing ancestor resolved, `..`
removed, a dangling symlink followed to where it points, up to 40 hops), then
matched in this order; the first match wins. A link chain that loops or cannot
be read is refused for every tool. The extension's `canonicalTargetPath` resolves
the same way, so its write pin agrees with Rust. A root is found among the
target's ancestors by case-insensitive equality or by filesystem identity
(`(dev, ino)`), and the rules below apply to the components after it, so a
firmlink or case alias of a root counts at whatever depth it spells the root.

1. Instructions: workspace `AGENTS.md`, `AGENTS.override.md`, `CLAUDE.md`,
   `.loupist/AGENTS.md`, `.loupist/config.toml`, Skill entries under `.loupist`,
   `.agents` and `.pi`, the global `AGENTS.md` and everything under the global
   `skills` folder.
2. Scratch: this run's `TMPDIR` (`<app data>/loupists/<id>/pi-runtime/<chat>/temp`).
3. Attachments: this Chat's attachment folder, which the host asks the model to read.
4. AppPrivate: anything else under the app data root (credentials, other Chats'
   runtime folders, `.pi/settings.json`) or under the Loupist's data folder
   `<workspace>/.loupist` (the database, other Chats' attachments, the activity
   log), except its `skills` folder.
5. Sensitive: `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.config/gcloud`, `~/.kube`,
   `~/.docker/config.json`, `~/.netrc`, `~/.git-credentials`, `~/.npmrc`, `~/.pypirc`,
   `~/Library/Keychains`, `~/Library/LaunchAgents`, shell startup files, `~/.gitconfig`,
   `~/.config/git/**`, `~/.config/gh/**`, and the `hooks/**` and `config` of any git
   directory (`.git`, a submodule's `.git/modules/<path>`, a bare `*.git`). One
   table in the module.
6. Workspace: inside the selected workspace.
7. Outside: everything else.

## Policy table

| Tool | Class | Clean | ThisTurn, Carried, Custom |
| --- | --- | --- | --- |
| read, ls | AppPrivate | Refuse | Refuse |
| read, ls | Sensitive | Ask `file_read` | Ask `file_read` |
| read, ls | Instructions, Scratch, Attachments, Workspace | Allow | Allow |
| read, ls | Outside | Allow | Ask `file_read` (folder, this reply) |
| find, grep | root AppPrivate, or root contains the app data root | Refuse | Refuse |
| find, grep | root Sensitive, or root holds a home Sensitive location (`~/.config`) | Ask `file_read` | Ask `file_read` |
| find, grep | otherwise | as read | as read |
| write, edit | AppPrivate, Attachments | Refuse | Refuse |
| write, edit | Sensitive | Ask `file_write` | Ask `file_write` |
| write, edit | Scratch, Workspace | Allow | Allow |
| write, edit | Instructions | Allow | Ask `instructions` (that file) |
| write, edit | Outside | Allow | Ask `file_write` (folder, this reply) |
| bash | | Allow | ThisTurn: Ask `command`. Carried: Ask `command` with a Chat scope. Custom: Refuse. |

Refusals carry a broker code the step shows: `protected_path` for AppPrivate and a
search above app data, `user_declined` after Not now on a file card, and
`untrusted_text_paused` after Not now on a command ([Chat turns](chat-turns.md)).

## Cards

`ChatApproval.kind` is `command`, `file_read`, `file_write` or `instructions`;
`connection_name` is "Workspace", `tool_name` the native tool, `target` the path or
the command's first non-blank line (at most 600 bytes), and `detail` the whole
command (at most 8 KiB). A command with no visible text shows "(blank command)";
zero-width and bidi controls, the byte order mark and other control characters
in a target or command show as `\u{…}`. The card wraps the command and shows its
line count beside it.

- `command` under ThisTurn: Allow once (this exact command, this reply), Allow
  commands for this reply, Not now.
- `command` under Carried: Allow once, Allow commands in this Chat (`grant: chat`;
  an in-memory grant keyed by Loupist and Chat, `chat/command_grants.rs`, cleared by
  the settled line's "Stop allowing" through `chat_command_grant_revoke` and by an
  app restart), Not now. Under ThisTurn the Chat grant does not apply.
- `file_read`, `file_write`: Allow (this reply), Not now. "Always" counts as once.
  Allow covers an outside file's folder, except a file directly in the home
  folder or at the root of the disk, which covers that file only; a directory
  target or a search root covers that directory only; a sensitive file covers
  that file only.
- `instructions`: Allow once (that file, this reply), Not now.
- Not now, a card the wait outran and a run stop decline that kind for the rest
  of the reply; later calls of that kind are refused without a card.
- A call that cannot show a card (an unattended run, no sink, or a reply already
  holding `MAX_PROGRESS_APPROVALS` cards) is refused on its own with
  `execution_failed` and a `next` that says why; the kind is not declined.

One `tokio::sync::Mutex` per ask kind: a call that decides Ask takes its kind's lock,
decides again (a card answered meanwhile may cover it) and only then asks, so
same-kind asks show one card at a time and different kinds ask concurrently.
Allowed calls never take a lock.

An allowed `write` or `edit` returns the canonical target as `resolved`; the
extension re-resolves the path right before writing and refuses on mismatch, so a
symlink swapped in after authorization cannot redirect the write.

## Residual risk

- Bash runs with the user's host permissions and no OS sandbox. A macOS seatbelt
  (no network, writes confined) would let a tainted turn run commands offline
  without a card; it needs macOS verification.
- `models.json` and `connections.json` are AppPrivate for native file tools, but a
  command can still write them. An integrity seal checked before launch is a
  follow-up.
- Bash output never raises the untrusted-text tier, and background commands can
  outlive the turn.
- A workspace search runs `rg --hidden` over the whole workspace, so it reaches
  the workspace's `.git` contents and the text files under `.loupist` (other
  Chats' attachments; the database is binary, which `rg` does not print) that a
  direct `read` would ask about or refuse. Only a root that holds a home
  Sensitive location asks.
