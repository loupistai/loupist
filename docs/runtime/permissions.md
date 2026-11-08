# Native tool permissions

What a native Pi tool (`read`, `ls`, `find`, `grep`, `write`, `edit`, `bash`) may do in one Chat reply, decided by Rust from the target's path class and whether the reply has read third-party text. Nothing asks the user: file tools work inside the Loupist folder, Bash runs inside an OS sandbox, and everything else is refused with a `next` that tells the model what to do instead.

The policy is one pure module,
`apps/loupist/src-tauri/src/agent_harness/native_policy.rs`; the sandbox is
`apps/loupist/src-tauri/src/agent_harness/sandbox.rs`. The extension
(`apps/loupist/agent/workspace-tools.mjs`) asks `native.authorize` before every
native execution with the resolved file (`read`, `write`, `edit`), the resolved search
root (`ls`, `find`, `grep`; the workspace by default) or the exact command (`bash`).
`AppToolExecutor::native_authorize` (`commands/tool_executor/workspace.rs`) parses the
request once and decides. Unattended runs refuse every native tool before this policy
(`briefings/policy.rs`).

## Exposure

| Exposure | When |
| --- | --- |
| Clean | No third-party text this reply. Earlier replies do not count: the user's new message is the checkpoint. |
| ThisTurn | This turn raised the untrusted-text tier ([Untrusted text](untrusted-text.md)). |
| Custom | This turn read a custom-connection result. File rows behave as ThisTurn; Bash is refused (`outbound_paused`). |

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
5. Credentials: `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.config/gcloud`, `~/.kube`,
   `~/.docker/config.json`, `~/.netrc`, `~/.git-credentials`, `~/.npmrc`, `~/.pypirc`,
   `~/.config/gh/**`, `~/.config/git/**`, `~/Library/Keychains` and the shell startup
   files (`.zshrc`, `.zprofile`, `.zshenv`, `.zlogin`, `.bashrc`, `.bash_profile`,
   `.profile`, `~/.config/fish/**`). One table (`CREDENTIAL_HOME`) serves this class
   and the sandbox's masks.
6. GitConfig: the `hooks/**` and `config` of any git directory (`.git`, a
   submodule's `.git/modules/<path>`, a bare `*.git`).
7. Workspace: inside the selected workspace, the Loupist folder.
8. Outside: everything else.

## Policy table

| Tool | Class | Clean | ThisTurn, Custom |
| --- | --- | --- | --- |
| read, ls | Instructions, Scratch, Attachments, GitConfig, Workspace | Allow | Allow |
| read, ls | AppPrivate, Credentials | Refuse `protected_path` | Refuse `protected_path` |
| read, ls | Outside | Refuse `outside_workspace` | Refuse `outside_workspace` |
| find, grep | root contains the app data root | Refuse `protected_path` | Refuse `protected_path` |
| find, grep | otherwise | as read | as read |
| write, edit | Scratch, Workspace | Allow | Allow |
| write, edit | Instructions | Allow | Refuse `untrusted_text_paused` |
| write, edit | AppPrivate, Attachments, Credentials, GitConfig | Refuse `protected_path` | Refuse `protected_path` |
| write, edit | Outside | Refuse `outside_workspace` | Refuse `outside_workspace` |
| bash | | Allow, online | ThisTurn: Allow offline inside a verified sandbox, else Refuse `untrusted_text_paused`. Custom: Refuse `outbound_paused`. |

Every refusal carries a `next` for the model. `outside_workspace` says to ask the
user to attach the file to the chat or copy it into the Loupist folder, and the
Chat step says the same to the user, as `protected_path` and
`untrusted_text_paused` steps already do ([Chat turns](chat-turns.md)). A search root
containing `.loupist` data is still allowed, so a workspace `grep` never refuses;
`rg` prints no binary database pages.

An allowed `write` or `edit` returns the canonical target as `resolved`; the
extension re-resolves the path right before writing and refuses on mismatch, so a
symlink swapped in after authorization cannot redirect the write.

## Bash sandbox

An allowed `bash` returns `sandbox: {argv, env}` when the host has a verified
sandbox. The extension runs the command through Pi's `BashOperations` as
`argv…, <shell>, -c, <command>`, each its own argument, with `env` added to the
allowlisted environment; a malformed `sandbox` throws instead of running the
command. Without `sandbox` in the answer (a clean reply on a host with none) the
command runs as before. Rust builds the prefix from the turn's roots:

| Rule | Linux (bubblewrap) | macOS (`/usr/bin/sandbox-exec -p`) |
| --- | --- | --- |
| Writes | Workspace and scratch bound read-write over a read-only `/`; a private tmpfs `/tmp`. | `(deny file-write*)` except the workspace, scratch and `/dev/null`, `/dev/stdout`, `/dev/stderr`, `/dev/dtracehelper`, `/dev/autofs_nowait`, `/dev/tty*`, `/dev/fd/*`. |
| `<workspace>/.loupist`, app data root | A tmpfs over each, remounted read-only last; `.loupist/skills`, `.loupist/AGENTS.md`, `.loupist/config.toml`, the global `skills` and `AGENTS.md`, this Chat's attachments and scratch bound back (attachments and instruction files read-only). | `(deny file-read*)` and `(deny file-write*)` on each with `require-not` exclusions for the same paths. |
| Workspace `.git/config`, `.git/hooks` | Bound read-only over themselves. | `(deny file-write*)`. |
| Credentials | Each existing `CREDENTIAL_HOME` entry: a folder gets a read-only tmpfs, a file `/dev/null`. | `(deny file-read*)` on each entry. |
| Network | `--unshare-net` after third-party text. | `(deny network*)` after third-party text. |
| Processes | `--unshare-pid --die-with-parent --new-session`: bubblewrap is the init of a pid namespace, so Pi's group kill on abort or timeout ends every sandboxed process. | The host's process rules; Pi kills the process group. |
| Caches | `XDG_CACHE_HOME`, `PIP_CACHE_DIR`, `UV_CACHE_DIR`, `npm_config_cache`, `MPLCONFIGDIR` point under `<scratch>/cache`. | Same. |

The macOS profile is `(allow default)` followed only by deny rules, each with
its own exclusions, so no allow and deny ever match the same operation and path
and nothing rests on rule precedence.

### Probe and fallback

At startup Rust probes the sandbox once per app run, prewarmed like the login
PATH (`sandbox::prewarm`): it lays out a temporary folder like a turn's roots,
opens a local listener, and runs `/bin/bash` under the sandbox twice. Offline, a
write inside the folder and scratch must succeed; a write outside, into the
data folder or into a git hook must fail; the app's `auth.json`, `~/.netrc`,
`~/.ssh` and the database must read as absent or empty; a Skill, an attachment
and a hook must still read; a connection to the listener must fail. Online the
connection must succeed, which proves the offline check measured the network.
Rust also checks the host folder afterwards. Only a passing probe enables the
sandbox; a turn that starts before the probe finishes waits for it off the async
workers, up to the probe's deadline.

Without a verified sandbox (a failed probe, no `bwrap`, Windows): a clean reply
runs Bash unconfined as before, and a reply that read third-party text refuses
Bash with `untrusted_text_paused`. Bash never runs unconfined after third-party
text.

## Residual risk

- The macOS profile is verified only by the startup probe until it is checked
  on a Mac; the Linux sandbox has integration tests against real bubblewrap
  (`sandbox.rs`, `workspace-tools.test.mjs`).
- In a clean reply Bash has the network, so a command can send what it reads.
  The reply's own text is the model's; third-party text turns the network off.
- Bash reads everything the user can read outside the masked set: the app data
  root, `.loupist` data and the `CREDENTIAL_HOME` table are hidden, other home
  files are not. `~/.gitconfig` stays readable so `git` keeps its identity.
- Only the workspace root's `.git/config` and `.git/hooks` are read-only for
  Bash; a nested repository's are writable like the rest of the workspace. File
  tools refuse every git directory's hooks and config.
- A Loupist folder that contains the home folder exposes its non-credential
  files to every tool and its credential locations to `find` and `grep`.
- Masks apply to the folders that exist when the command starts; the file tools
  classify by path regardless.
- `models.json` and `connections.json` are AppPrivate for native file tools and
  hidden from Bash; a clean reply's unconfined command on a host without a
  sandbox can still write them.
- Bash output never raises the untrusted-text tier, and a background command can
  outlive the turn, inside the sandbox.
- Pi's `read` falls back to curly-quote, NFD and U+202F spellings of a missing
  path, so the file it opens can differ from the one Rust classified. Rust
  classifies the path the extension sends, not the fallback.
