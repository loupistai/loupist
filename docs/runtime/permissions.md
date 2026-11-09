# Native tool permissions

What a native file tool (`read`, `ls`, `find`, `grep`, `write`, `edit`) and `run_python` may do in one Chat reply. Rust decides file calls from the target's path class and whether the reply has read third-party text. Nothing asks the user. File tools work inside the admitted folders. `run_python` stays allowed after third-party text and in unattended runs, without a permission card. Disallowed file calls are refused with a `next` that tells the model what to do instead.

The file policy is one pure module,
`apps/loupist/src-tauri/src/agent_harness/native_policy.rs`. The extension
(`apps/loupist/agent/workspace-tools.mjs`) asks `native.authorize` before every
native file execution with the resolved file (`read`, `write`, `edit`) or the resolved search
root (`ls`, `find`, `grep`; the workspace by default).
`AppToolExecutor::native_authorize` (`commands/tool_executor/workspace.rs`) parses the
request once and decides. Unattended runs refuse native file tools before this policy
(`briefings/policy.rs`). `run_python` is not part of that refusal.

There is no agent Bash tool. Model code does not get a shell, the host Python, or a JavaScript runtime.

## Exposure

| Exposure | When |
| --- | --- |
| Clean | No third-party text admitted in this reply yet. Returned history or retained material can raise the tier. |
| ThisTurn | This turn raised the untrusted-text tier ([Untrusted text](untrusted-text.md)). |
| Custom | This turn read a custom-connection result. File rows behave as ThisTurn. `run_python` stays allowed. |

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
7. Workspace: inside the selected workspace, the Loupist folder. A default
   Loupist is created inside the app-data namespace: its strictly nested selected
   workspace counts as Workspace, while its own `.loupist` store, other Loupists,
   runtime folders and the rest of app data remain private. Selecting the app-data
   root itself or a parent never grants access to its private contents.
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
| run_python | | Allow, no card, including unattended runs and both exposure tiers. | Allow |

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
`run_python` runs in a separate worker process. It does not share the Pi process with file tools.

## Python execution

`run_python` is broker method `python.run` with permission always. The model sends `code` and an optional `timeout_seconds`. Code is at most 64 KiB. The timeout defaults to 60 seconds and cannot exceed 120. The worker is `loupist-python-worker` in the staged `python-runtime` resource. It hosts Wasmtime 50.0.1 and the pinned CPython WASI snapshot in `apps/loupist/python-worker/runtime.json` (ClayGendron/wasi-python v0.1.4). The host does not invoke native Python for model code.

Each call snapshots the allowed workspace files, this Chat's attachments and a scratch directory into a private host-owned tree. The guest sees `/workspace` as its relative cwd, `/attachments` for this Chat as read-only, `/scratch` for this run as writable, and `/usr/local/lib` for the runtime as read-only. Matplotlib's data directory is a separate read-only preopen. The snapshot omits dot directories recursively, app private roots, credentials, `.git`, symlinks, hard links, special files and nested instruction locations. Top-level workspace files are read-only. Writes are granted only for `outputs/`, which the host creates, and for existing non-dot top-level subdirectories the host allows. The worker never receives a live host directory as a writable WASI preopen.

The worker has no network and no DNS. TCP, UDP and IP name lookup are disabled, and every socket address check fails. It does not inherit the host environment. The guest environment is only `HOME`, `TMPDIR`, `MPLCONFIGDIR`, `MPLBACKEND`, `PYTHONTZPATH` and `OPENBLAS_NUM_THREADS`. There is no shell and no process spawn. The module's `host.call` import returns failure for every argument. Linear memory is limited to 512 MiB. The parent caps captured output at 64 KiB. The parent kills and reaps the child on cancellation and on the wall timeout, including a blocked sleep or a module load, and the worker exits if its parent dies. Only one Python call runs at a time in an app process; queued calls can be cancelled and wait at most their requested timeout. A broker disconnect cancels the host-owned run without dropping its cleanup or file copy-back.

The offline packages are numpy, pandas, scipy, matplotlib, statsmodels and scikit-learn. The guest cannot install a package or fetch one. macOS arm64 is the release host. Linux is a development host. Both use the same wasm snapshot and a worker built for that machine.

On macOS the worker is signed with the hardened runtime and `com.apple.security.cs.allow-unsigned-executable-memory`. Wasmtime 50 maps executable memory with W^X `mmap` rather than Apple's `MAP_JIT`, so that entitlement is required for the worker to run. The Loupist app binary does not receive it. Pi keeps its own entitlement file for the Node runtime. That file is not applied to the app.

Rust copies allowed guest writes back to the host, including writes made before cancellation or timeout. Copy-back reapplies the native path policy to every destination, including newly created names, and admits only ordinary workspace content. It uses descriptor-relative nofollow and openat checks and refuses symlink races, replaced parents and hard-link aliases. The host does not extract guest archives onto itself. If a host file changes during the call, copy-back does not overwrite it. The snapshot is limited to 2,000 files and 32 MiB; copy-back is limited to 256 changed files and 32 MiB, with an 8 MiB limit per file. Walks visit at most 10,000 entries and 32 directory levels; at most 256 writable roots, including `outputs`, are admitted. Oversized output files and changed host files are reported individually so other allowed results survive. Removing or renaming an existing directory is reported as a conflict and leaves its original host subtree intact; remove individual files when needed.

## Residual risk

- Python can modify files in an authorized workspace subfolder. That includes `outputs/` and any existing non-dot top-level directory the host grants. A file there may be instructions for another program, such as a build script or a hook outside `.git`. This boundary does not make those programs safe to run later.
- The worker's hardened runtime still allows unsigned executable memory. A bug in Wasmtime, Cranelift or the WASI engine can use that mapping. The entitlement is on the worker binary only. It is not on the Loupist app.
- The CPython standard library inside the sandbox can still read the granted preopens and can spend the memory and time budgets. Those budgets do not stop an engine bug.
- The 512 MiB cap covers guest linear memory, not the worker's resident size. Compiling the fixed runtime adds host memory and CPU costs; the private compiled-module cache reduces subsequent startup costs. Calls run serially to prevent parallel compilers multiplying that overhead. The snapshot and copy-back byte limits do not impose a live guest disk quota: temporary writes can use disk space until the call ends and its private tree is removed.
- File changes that the host copies back survive cancellation. Path checks and execution are separate operations. Another Chat or a local process can change a path during the call. Copy-back refuses a host file whose contents changed. It does not freeze the rest of the machine.
- Hard-link checks on file tools run at authorization. Another local actor can change link topology afterwards. Descriptor checks also refuse a newly multiply-linked file.
- A Python snapshot containing exact bytes tagged as third-party exports raises the untrusted-text tier when its result is delivered, and its stdout/stderr are delimited. Without guest read tracing this conservatively applies even if the program did not read that file. Derived copied-back files receive content-addressed tags before publication, so later reads preserve the boundary. Ordinary calculation output and unclassified local files do not raise the tier. A new reply starts clean unless admitted history or retained results raise it. The user's next message does not erase injected text from conversation history.
- The web site rule limits direct `web_fetch` targets. It is not a general exfiltration firewall. See [Untrusted text](untrusted-text.md).
