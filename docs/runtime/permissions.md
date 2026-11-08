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
target's ancestors by exact spelling or by the filesystem identity (`(dev,
ino)`) of an existing folder, and the rules below apply to the components after
it. A firmlink or a case alias of a root on a case-insensitive volume therefore
counts at whatever depth it spells the root, while on a case-sensitive volume
`~/RESEARCH` is another folder, or none, and is Outside. Names inside a root
(`AGENTS.md`, `skills`, `.ssh`, `.git`) compare without case.

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
6. GitDir: inside the workspace, any path with a `.git` component (the `.git`
   file or folder itself and everything in it, a nested repository's too) and
   the `hooks/**` and `config` of a bare `*.git` or a submodule's
   `.git/modules/<path>`. Another repository's git directory is Outside.
7. Workspace: inside the selected workspace, the Loupist folder.
8. Outside: everything else.

## Policy table

| Tool | Class | Clean | ThisTurn, Custom |
| --- | --- | --- | --- |
| read, ls | Instructions, Scratch, Attachments, GitDir, Workspace | Allow | Allow |
| read, ls | AppPrivate, Credentials | Refuse `protected_path` | Refuse `protected_path` |
| read, ls | Outside | Refuse `outside_workspace` | Refuse `outside_workspace` |
| find, grep | root contains the app data root | Refuse `protected_path` | Refuse `protected_path` |
| find, grep | otherwise | as read | as read |
| write, edit | Scratch, Workspace | Allow | Allow |
| write, edit | Instructions | Allow | Refuse `untrusted_text_paused` |
| write, edit | AppPrivate, Attachments, Credentials, GitDir | Refuse `protected_path` | Refuse `protected_path` |
| write, edit | Outside | Refuse `outside_workspace` | Refuse `outside_workspace` |
| bash | | Allow, online | ThisTurn: Allow offline inside a verified sandbox, else Refuse `untrusted_text_paused`. Custom: Refuse `outbound_paused`. |

Every refusal carries a `next` for the model. `outside_workspace` says to ask the
user to attach the file to the chat or copy it into the Loupist folder, and the
Chat step says the same to the user, as `protected_path` and
`untrusted_text_paused` steps already do ([Chat turns](chat-turns.md)). A search root
containing `.loupist` data is still allowed, so a workspace `grep` never refuses;
`rg` prints no binary database pages.

An allowed file call returns the canonical target as `resolved`. The extension
re-resolves a `write` or `edit` path right before writing and refuses on
mismatch, so a symlink swapped in after authorization cannot redirect the
write. A `read` answer also carries the file's `identity` (`{dev, ino}` as
strings, `null` when no file is there yet): the extension opens `resolved`,
compares the descriptor's identity and reads through that descriptor, so a
link swapped in after authorization reads nothing, and a path with no file at
authorization never reads a file that appears later. `ls`, `find` and `grep`
run on `resolved`, the canonical root Rust decided about. In the extension,
native file tools and Bash never run at the same time: Bash holds a
per-process lock alone, file tools share it, and waiters go in order, so Pi's
parallel tool calls cannot race a command against a read.

## Bash sandbox

An allowed `bash` returns `sandbox: {argv, env}` when the host has a verified
sandbox. The extension runs the command through Pi's `BashOperations` as
`argv…, <shell>, -c, <command>`, each its own argument, with `env` added to the
allowlisted environment; a malformed `sandbox` throws instead of running the
command. Without `sandbox` in the answer (a clean reply on a host with none) the
command runs as before. Rust builds the prefix from the turn's roots. Linux
(bubblewrap) is the only sandbox; see [Probe and fallback](#probe-and-fallback)
for every other host.

| Rule | Linux (bubblewrap) |
| --- | --- |
| Writes | Workspace and scratch bound read-write over a read-only `/`; a private tmpfs `/tmp`. |
| Runtime sockets | A tmpfs over `/run` (and `/var/run` when it is a folder of its own), so `XDG_RUNTIME_DIR` sockets (D-Bus session bus, ssh-agent, gpg-agent, PulseAudio, Wayland), `/run/dbus/system_bus_socket` and `docker.sock` are gone. When `/etc/resolv.conf` resolves into `/run`, that file is bound back read-only so names still resolve online. |
| `<workspace>/.loupist`, app data root | A tmpfs over each, remounted read-only last; `.loupist/skills`, `.loupist/AGENTS.md`, `.loupist/config.toml`, the global `skills` and `AGENTS.md`, this Chat's attachments and scratch bound back (attachments and instruction files read-only). |
| Workspace `.git` | A `.git` folder is bound onto itself, so it cannot be renamed or removed, and its `config` and `hooks` are bound read-only over themselves; a `.git` file (a worktree's gitdir pointer) is bound read-only. |
| Credentials | Each existing `CREDENTIAL_HOME` entry: a folder gets a read-only tmpfs, a file `/dev/null`. |
| Network | `--unshare-net` after third-party text. In a clean reply the command shares the host's network namespace, abstract Unix sockets included. |
| Processes | `--unshare-pid --die-with-parent --new-session`: bubblewrap is the init of a pid namespace, so Pi's group kill on abort or timeout ends every sandboxed process, and a background process the command leaves behind ends with the command. |
| Caches | `XDG_CACHE_HOME`, `PIP_CACHE_DIR`, `UV_CACHE_DIR`, `npm_config_cache`, `MPLCONFIGDIR` point under `<scratch>/cache`. |

### Probe and fallback

At startup Rust probes the sandbox once per app run, prewarmed like the login
PATH (`sandbox::prewarm`): it lays out a private folder under the user's cache
folder (`$XDG_CACHE_HOME` or `~/.cache`, under `loupist/`) like a turn's roots,
opens a local listener, and runs `/bin/bash` under the sandbox twice. The
layout is not under `/tmp`, which the sandbox replaces with an empty tmpfs: a
secret there would read as absent with or without the masks, so a sandbox that
lost them would still pass. Offline, a write inside the folder and scratch must
succeed; a write outside, into the data folder or into a git hook must fail;
the app's `auth.json`, `~/.netrc`, `~/.ssh` and the database must read as
absent or empty; a Skill, an attachment and a hook must still read; a connection
to the listener must fail. Online the connection must succeed, which proves the
offline check measured the network. Rust also checks the host folder
afterwards. Only a passing probe enables the sandbox; a failed one logs the
checks that failed and, when the sandbox never started, its own first error
line; a turn that starts before the probe finishes waits for it off the async
workers, up to the probe's deadline.

Without a verified sandbox (a failed probe, no `bwrap`, macOS, Windows): a
clean reply runs Bash unconfined as before, and a reply that read third-party
text refuses Bash with `untrusted_text_paused` and no card. Bash never runs
unconfined after third-party text.

macOS has no sandbox yet. A `sandbox-exec` profile that started from
`(allow default)` was removed: it left LaunchServices, AppleEvents and XPC
(`open -a Terminal`, `osascript`) able to start unconfined programs with the
network, which would have made tainted Bash worse than this fallback on the
release platform. The follow-up is a deny-default seatbelt profile verified on a
Mac, with sandbox-runtime's macOS profile as the reference.

## Residual risk

- In a clean reply Bash has the network, so a command can send what it reads.
  The reply's own text is the model's; third-party text turns the network off.
- In a clean reply the sandbox shares the host's network namespace, so services
  on abstract Unix sockets (X11, some D-Bus setups) stay reachable; offline mode
  removes them with the rest of the network.
- Bash reads everything the user can read outside the masked set: the app data
  root, `.loupist` data, the `CREDENTIAL_HOME` table and `/run` are hidden,
  other home files are not. `~/.gitconfig` stays readable so `git` keeps its
  identity.
- Only the workspace root's `.git` is pinned and read-only for Bash; a nested
  repository's `.git`, or a `.git` Bash creates in a subfolder, is writable
  like the rest of the workspace, and the host's `git` would run its hooks
  there. File tools refuse every git directory inside the workspace.
- A Loupist folder that contains the home folder exposes its non-credential
  files to every tool and its credential locations to `find` and `grep`.
- Masks apply to the folders that exist when the command starts; the file tools
  classify by path regardless.
- `models.json` and `connections.json` are AppPrivate for native file tools and
  hidden from Bash; a clean reply's unconfined command on a host without a
  sandbox can still write them.
- Bash output never raises the untrusted-text tier.
- A workspace `grep` can print text from `.loupist` files Rust itself would
  refuse to read, such as the activity log; `rg` prints no binary database
  pages.
- Pi's `read` falls back to curly-quote, NFD and U+202F spellings of a missing
  path. Rust classifies the path the extension sends, and the extension opens
  the file Rust decided about by identity, so a fallback spelling never opens
  another file; a read of a missing path fails even when a fallback spelling
  exists.
- The next reply starts clean: the user's new message is the checkpoint, and
  nothing this reply read carries its exposure forward.
