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
   `~/.netrc`, `~/.git-credentials`, `~/.npmrc`, `~/.pypirc`, `~/.config/gh/**`,
   `~/.config/git/**`, `~/Library/Keychains`, the shell startup files (`.zshrc`,
   `.zprofile`, `.zshenv`, `.zlogin`, `.bashrc`, `.bash_profile`, `.profile`,
   `~/.config/fish/**`) and the container runtimes' folders, whole: `~/.docker`
   (its `config.json` credentials and Docker Desktop's `desktop/docker.sock`),
   `~/.colima`, `~/.lima` and `~/.rd` (Rancher Desktop). One table
   (`CREDENTIAL_HOME`) serves this class and the sandbox's masks.
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
| bash | | Allow, online | ThisTurn: Allow offline inside a verified sandbox with the workspace [read-only except its subfolders](#after-third-party-text), else Refuse `untrusted_text_paused`. Custom: Refuse `outbound_paused`. |

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
| Writes, clean reply | Workspace and scratch bound read-write over a read-only `/`; a private tmpfs `/tmp`. |
| Writes, after third-party text | The workspace bound read-only, then its existing top-level subfolders, `<workspace>/outputs` and scratch bound back read-write ([After third-party text](#after-third-party-text)); a private tmpfs `/tmp`. |
| Runtime sockets | A tmpfs over `/run` (and `/var/run` when it is a folder of its own), so `XDG_RUNTIME_DIR` sockets (D-Bus session bus, ssh-agent, gpg-agent, PulseAudio, Wayland), `/run/dbus/system_bus_socket` and `/run/docker.sock` are gone. When `/etc/resolv.conf` resolves into `/run`, that file is bound back read-only so names still resolve online. The container runtimes' home folders (`~/.docker`, `~/.colima`, `~/.lima`, `~/.rd`) are hidden with the credentials, in both modes. Any other Unix socket under the home folder stays reachable in both modes: offline mode removes the network and abstract sockets only, not filesystem sockets. |
| `<workspace>/.loupist`, app data root | A tmpfs over each, remounted read-only last; `.loupist/skills`, `.loupist/AGENTS.md`, `.loupist/config.toml`, the global `skills` and `AGENTS.md`, this Chat's attachments and scratch bound back (attachments and instruction files read-only). |
| Workspace `.git`, clean reply | A `.git` folder is bound onto itself, so it cannot be renamed or removed, and its `config` and `hooks` are bound read-only over themselves; a `.git` file (a worktree's gitdir pointer) is bound read-only. The rest of `.git` is writable. |
| Workspace `.git`, after third-party text | Part of the read-only workspace: nothing in it can be created, changed, renamed or removed. |
| Credentials | Each existing `CREDENTIAL_HOME` entry: a folder gets a read-only tmpfs, a file `/dev/null`. |
| Network | `--unshare-net` after third-party text. In a clean reply the command shares the host's network namespace, abstract Unix sockets included. |
| Processes | `--unshare-pid --die-with-parent --new-session`: bubblewrap is the init of a pid namespace, so Pi's group kill on abort or timeout ends every sandboxed process, and a background process the command leaves behind ends with the command. |
| Caches | `XDG_CACHE_HOME`, `PIP_CACHE_DIR`, `UV_CACHE_DIR`, `npm_config_cache`, `MPLCONFIGDIR` point under `<scratch>/cache`. |

### After third-party text

A command that runs after this reply read third-party text may act on
injected text. Native file tools refuse its writes to the files a later reply
loads as instructions; the mount gives commands a stronger version of the same
rule, with nothing for the host to repair afterwards. Rust binds the workspace
read-only, then binds read-write every existing top-level entry of the
workspace that is a folder itself (`symlink_metadata`, so a link to a folder
does not count) and whose name does not start with `.`, plus
`<workspace>/outputs`, which Rust creates before the command when it is
missing, plus scratch. So a command cannot create, change, rename or remove
anything at the workspace's top level (`AGENTS.md`, `AGENTS.override.md`,
`CLAUDE.md`, a new folder), in any dot entry (`.git`, `.agents`, `.pi`,
`.loupist`, any other) or through a link or hard link into them, while work
inside existing folders and in `outputs/` goes on as before. A top-level
folder is a mount point, so it cannot be renamed or removed either; its
contents can.

The number of folders bound back is capped at `MAX_WRITABLE_CHILDREN` (256).
A workspace with more top-level subfolders keeps only `outputs` and scratch
writable for the command. A folder whose name is not valid UTF-8 cannot be
passed to bubblewrap and stays read-only. When `outputs` exists as a file or
a link, it is not bound and nothing is written there.

A clean reply's command keeps the clean-mode layout, so its instruction edits
are the model's own and stay. Nothing is snapshotted or restored at reply end:
what a command after third-party text could not write, it did not write.

### Probe and fallback

At startup Rust probes the sandbox once per app run, prewarmed like the login
PATH (`sandbox::prewarm`): it lays out a private folder under the user's cache
folder (`$XDG_CACHE_HOME` or `~/.cache`, under `loupist/`) like a turn's roots,
opens a local listener, and runs `/bin/bash` under the sandbox twice. The
layout is not under `/tmp`, which the sandbox replaces with an empty tmpfs: a
secret there would read as absent with or without the masks, so a sandbox that
lost them would still pass. In both modes a write into an existing subfolder,
into `outputs` (which the offline run must have created) and into scratch must
succeed; a write outside, into the data folder or into a git hook must fail;
`mv .git` must fail; the app's `auth.json`, `~/.netrc`, `~/.ssh` and the
database must read as absent or empty; a Skill, an attachment and a hook must
still read; and `/run` must hold nothing but the resolver file bound back.
Offline, a write at the folder's top level and into a dot folder
(`.agents/skills`) must fail and a connection to the listener must fail; online
both must succeed, which proves the offline checks measured the mount and the
network. Rust also checks the host folder afterwards: the top-level file exists
only after the online run, the subfolder file after both, the outside file
never, and `.git/hooks/pre-commit` is still in place. Only a passing probe
enables the sandbox; a failed one logs the checks that failed and, when the
sandbox never started, its own first error line; a turn that starts before the
probe finishes waits for it off the async workers, up to the probe's deadline.

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
- A Unix socket a user keeps under the home folder outside the hidden set (a
  daemon configured to listen under `~/.local`, for example) is reachable in
  both modes, and a daemon behind it acts outside the sandbox with whatever
  the daemon has. The known container-runtime socket folders are hidden; the
  rest of the home folder is not.
- In a clean reply only the root `.git` entry, its `config` and its `hooks`
  are read-only for Bash. The other files in `.git` are writable, and some of
  them redirect the host's `git` to another directory whose config and hooks
  it then runs: `commondir`, `config.worktree`, `worktrees/*`,
  `info/attributes` with a filter. A clean reply's command is the model's own
  work, and after third-party text `.git` takes no write at all. A nested
  repository's `.git`, or a `.git` Bash creates inside an existing subfolder,
  is writable in both modes like the folder around it, and the host's `git`
  would run its hooks there. File tools refuse every git directory inside the
  workspace.
- In a clean reply a command can create or change the workspace's instruction
  files and Skills; they are the model's own and nothing undoes them. A reply
  that read third-party text is followed by a clean one once the user sends
  the next message, and that clean reply's commands are online and may write
  everything again; what carries over is only what the earlier reply could
  write, which excludes every instruction file.
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
