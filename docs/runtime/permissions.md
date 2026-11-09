# Native tool permissions

What a native Pi tool (`read`, `ls`, `find`, `grep`, `write`, `edit`, `bash`) may do in one Chat reply, decided by Rust from the target's path class and whether the reply has read third-party text. Nothing asks the user: file tools work inside the admitted folders, Bash runs with host permissions only before third-party text, and disallowed calls are refused with a `next` that tells the model what to do instead.

The policy is one pure module,
`apps/loupist/src-tauri/src/agent_harness/native_policy.rs`. The extension
(`apps/loupist/agent/workspace-tools.mjs`) asks `native.authorize` before every
native execution with the resolved file (`read`, `write`, `edit`), the resolved search
root (`ls`, `find`, `grep`; the workspace by default) or the exact command (`bash`).
`AppToolExecutor::native_authorize` (`commands/tool_executor/workspace.rs`) parses the
request once and decides. Unattended runs refuse every native tool before this policy
(`briefings/policy.rs`).

## Exposure

| Exposure | When |
| --- | --- |
| Clean | No third-party text admitted in this reply yet. Returned history or retained material can raise the tier. |
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

1. Credentials: `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.config/gcloud`, `~/.kube`,
   `~/.netrc`, `~/.git-credentials`, `~/.npmrc`, `~/.pypirc`, `~/.config/gh/**`,
   `~/.config/git/**`, `~/Library/Keychains`, the shell startup files (`.zshrc`,
   `.zprofile`, `.zshenv`, `.zlogin`, `.bashrc`, `.bash_profile`, `.profile`,
   `~/.config/fish/**`) and the container runtimes' folders, whole: `~/.docker`
   (its `config.json` credentials and Docker Desktop's `desktop/docker.sock`),
   `~/.colima`, `~/.lima` and `~/.rd` (Rancher Desktop). One table
   (`CREDENTIAL_HOME`) defines this class. Both the home spelling and its
   canonical target are protected, including a store symlinked into the workspace.
2. GitDir: inside the workspace, any path with a `.git` component (the `.git`
   file or folder itself and everything in it, a nested repository's too) and
   the `hooks/**` and `config` of a bare `*.git` or a submodule's
   `.git/modules/<path>`. Another repository's git directory is Outside.
3. Instructions: workspace `AGENTS.md`, `AGENTS.override.md`, `CLAUDE.md`,
   `.loupist/AGENTS.md`, `.loupist/config.toml`, Skill entries under `.loupist`,
   `.agents` and `.pi`, the global `AGENTS.md` and everything under the global
   `skills` folder.
4. Scratch: this run's `TMPDIR` (`<app data>/loupists/<id>/pi-runtime/<chat>/temp`).
5. Attachments: this Chat's attachment folder, which the host asks the model to read.
6. AppPrivate: anything else under the app data root (credentials, other Chats'
   runtime folders, `.pi/settings.json`) or under the Loupist's data folder
   `<workspace>/.loupist` (the database, other Chats' attachments, the activity
   log), except its `skills` folder.
7. Workspace: inside the selected workspace, the Loupist folder.
8. Outside: everything else.

## Policy table

| Tool | Class | Clean | ThisTurn, Custom |
| --- | --- | --- | --- |
| read, ls | Instructions, Scratch, Attachments, Workspace | Allow | Allow |
| read, ls | AppPrivate, Credentials, GitDir | Refuse `protected_path` | Refuse `protected_path` |
| read, ls | Outside | Refuse `outside_workspace` | Refuse `outside_workspace` |
| find, grep | root contains the app data root | Refuse `protected_path` | Refuse `protected_path` |
| find, grep | otherwise | as read | as read |
| write, edit | Scratch, Workspace | Allow | Allow |
| write, edit | Scratch, Workspace: an existing file with another hard link | Refuse `protected_path` | Refuse `untrusted_text_paused` |
| write, edit | Instructions | Allow | Refuse `untrusted_text_paused` |
| write, edit | AppPrivate, Attachments, Credentials, GitDir | Refuse `protected_path` | Refuse `protected_path` |
| write, edit | Outside | Refuse `outside_workspace` | Refuse `outside_workspace` |
| bash | | Allow, online | ThisTurn: Refuse `untrusted_text_paused`. Custom: Refuse `outbound_paused`. |

A file whose link count is above 1 has another name on disk, and the classes
see only the name the call used: `notes/alias.md` hard-linked to `AGENTS.md`
is Workspace. After third-party text its `write` and `edit` are refused with a
`next` that says to write a new file instead. Other access to multiply linked
files is refused with `protected_path`: another name could be a credential or
app database file. Ordinary single-link files are unaffected.

Every refusal carries a `next` for the model. `outside_workspace` says to ask the
user to attach the file to the chat or copy it into the Loupist folder, and the
Chat step says the same to the user, as `protected_path` and
`untrusted_text_paused` steps already do ([Chat turns](chat-turns.md)). A search rooted at the workspace is allowed, but Rust builds a bounded list of
individually admitted entries before listing or matching. Private subtrees, git
metadata, links and special files never enter that list. Local ignore rules come
only from admitted regular `.gitignore` and `.ignore` files; host git configuration
and `.git/info/exclude` are not read. `node_modules` is omitted. Traversal stops at
10,000 visited entries, 2 MiB of metadata, depth 64 or five seconds and reports
truncation. Narrow the search root when the list is truncated.

An allowed file call returns the canonical target as `resolved`. Reads, writes
and edits also carry the file's `identity` (`{dev, ino}` as strings, `null` when
no file exists). The extension verifies the opened descriptor's identity, regular
file type and link count before reading or truncating; it refuses a last-component
symlink. A new write uses exclusive creation. Before creating parents it checks
the canonical parent against the approved path, so a swapped link cannot create
an outside directory. `ls` and `find` use the admitted list. `grep` opens only listed
files, checks each descriptor and passes it to `rg` as stdin, with no filesystem
search arguments or ambient configuration. Its output is capped at 50 KiB.
File tools and Bash never run together in the same Pi process: Bash holds an
exclusive lock, file tools share it, and waiters go in order.

## Bash execution

Before third-party text, Bash runs with the user's host permissions, as before
this change. The selected workspace is its starting directory. The child receives
an allowlisted environment without app credentials or shell startup hooks; this
does not restrict its access to host files, sockets, processes or the network.
After third-party text Rust refuses Bash with `untrusted_text_paused`, without a
permission card or override, on every host. The custom-connection tier retains
`outbound_paused`. There is no OS sandbox, probe, sandbox response or fallback.

## Residual risk

- Clean Bash can read or change any host file the OS user can access, including
  app data and credentials, and can use the network or start processes. File-tool
  policy does not confine Bash. This is the pre-PR execution boundary.
- File changes are immediate and survive cancellation. Writable workspace files
  may be instructions for another program, such as a build script or hook outside
  `.git`; this policy does not make those programs safe to run later.
- Path authorization and execution are separate operations. The read descriptor
  identity and per-process tool lock prevent same-process Bash/read races; another
  Chat or a local process can still mutate paths concurrently.
- Hard-link write checks run at authorization; another local actor can change link
  topology afterwards. Descriptor checks also refuse a newly multiply-linked file.
- Bash output and unclassified local files do not raise the untrusted-text tier.
  A new reply starts clean unless admitted history or retained results raise it.
  The user's next message does not erase injected text from conversation history.
- The web site rule limits direct `web_fetch` targets; it is not a general
  exfiltration firewall. See [Untrusted text](untrusted-text.md).
