//! What a Pi native tool (`read`, `ls`, `find`, `grep`, `write`, `edit`,
//! `bash`) may do in one Chat reply, decided from the target's path class and
//! how much third-party text the model has seen (`docs/runtime/permissions.md`).
//! Pure: the caller owns the locks, the cards and where grants are kept.

use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

use serde_json::Value;

use crate::chat::approvals::ApprovalDecision;

const MAX_PATH_BYTES: usize = 8 * 1024;
const MAX_COMMAND_BYTES: usize = 64 * 1024;
/// The full command a card shows; a longer command ends with an ellipsis.
pub const MAX_DETAIL_BYTES: usize = 8 * 1024;
/// A card's one-line target: a command's first line or a path.
const MAX_TARGET_CHARS: usize = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeTool {
    Read,
    Ls,
    Find,
    Grep,
    Write,
    Edit,
    Bash,
}

impl NativeTool {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "read" => Self::Read,
            "ls" => Self::Ls,
            "find" => Self::Find,
            "grep" => Self::Grep,
            "write" => Self::Write,
            "edit" => Self::Edit,
            "bash" => Self::Bash,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Ls => "ls",
            Self::Find => "find",
            Self::Grep => "grep",
            Self::Write => "write",
            Self::Edit => "edit",
            Self::Bash => "bash",
        }
    }

    fn writes(self) -> bool {
        matches!(self, Self::Write | Self::Edit)
    }

    fn searches(self) -> bool {
        matches!(self, Self::Find | Self::Grep)
    }
}

/// One `native.authorize` call, parsed once at the broker boundary. A file
/// tool names its resolved target (the search root for `ls`, `find` and
/// `grep`); `bash` names the exact command Pi will run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRequest {
    pub tool: NativeTool,
    pub target: Option<PathBuf>,
    pub command: Option<String>,
}

impl NativeRequest {
    /// `{name, path?, command?}`; an unknown name, a missing or oversized
    /// field, a NUL byte or an extra field is malformed.
    pub fn parse(params: &Value) -> Option<Self> {
        let object = params.as_object()?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "name" | "path" | "command"))
        {
            return None;
        }
        let tool = NativeTool::parse(object.get("name")?.as_str()?)?;
        let text = |key: &str, max: usize| -> Option<Option<String>> {
            match object.get(key) {
                None => Some(None),
                Some(value) => {
                    let text = value.as_str()?;
                    (!text.is_empty() && text.len() <= max && !text.contains('\0'))
                        .then(|| Some(text.to_owned()))
                }
            }
        };
        let path = text("path", MAX_PATH_BYTES)?;
        let command = text("command", MAX_COMMAND_BYTES)?;
        let request = match tool {
            NativeTool::Bash => Self {
                tool,
                target: None,
                command: Some(command?),
            },
            _ => Self {
                tool,
                target: Some(PathBuf::from(path?)),
                command: None,
            },
        };
        Some(request)
    }
}

/// The folders one turn's policy is decided against, each with its existing
/// part canonicalized. `home` is absent when the host has no home directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRoots {
    /// The selected workspace.
    pub workspace: PathBuf,
    /// The global instruction folder (`AGENTS.md` and `skills/**` live here).
    pub global: PathBuf,
    /// The app data root: credentials, Pi runtime files.
    pub app_data: PathBuf,
    /// This Loupist's data folder (`<workspace>/.loupist`): its database,
    /// attachments and activity log, beside the instruction files it holds.
    pub loupist_data: PathBuf,
    /// This run's `TMPDIR` under the Chat's Pi runtime folder.
    pub scratch: PathBuf,
    /// This Chat's attachment folder, which the host asks the model to read.
    pub attachments: PathBuf,
    pub home: Option<PathBuf>,
}

impl NativeRoots {
    pub fn new(
        workspace: &Path,
        global: &Path,
        app_data: &Path,
        loupist_data: &Path,
        scratch: &Path,
        attachments: &Path,
        home: Option<&Path>,
    ) -> Self {
        Self {
            workspace: resolved(workspace),
            global: resolved(global),
            app_data: resolved(app_data),
            loupist_data: resolved(loupist_data),
            scratch: resolved(scratch),
            attachments: resolved(attachments),
            home: home.map(resolved),
        }
    }
}

/// First match wins, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathClass {
    /// A file a later turn loads as instructions or a Skill, plus the
    /// workspace's `.loupist/config.toml`.
    Instructions,
    /// This run's temporary folder.
    Scratch,
    /// This Chat's attachments: readable, never written.
    Attachments,
    /// Anything else under the app data root or the Loupist's data folder.
    AppPrivate,
    /// Credential and startup files anywhere on the host.
    Sensitive,
    Workspace,
    Outside,
    /// A symlink chain that loops or cannot be read: no tool touches it.
    Unresolvable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classified {
    pub class: PathClass,
    /// The canonical target the decision was made about.
    pub path: PathBuf,
}

pub fn classify(path: &Path, roots: &NativeRoots) -> Classified {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        roots.workspace.join(path)
    };
    let Some(path) = resolve_links(&path) else {
        return Classified {
            class: PathClass::Unresolvable,
            path: lexical(&path),
        };
    };
    let class = if instruction_file(&path, roots) {
        PathClass::Instructions
    } else if under(&path, &roots.scratch) {
        PathClass::Scratch
    } else if under(&path, &roots.attachments) {
        PathClass::Attachments
    } else if under(&path, &roots.app_data) || loupist_private(&path, roots) {
        PathClass::AppPrivate
    } else if sensitive(&path, roots.home.as_deref()) {
        PathClass::Sensitive
    } else if under(&path, &roots.workspace) {
        PathClass::Workspace
    } else {
        PathClass::Outside
    };
    Classified { class, path }
}

/// The Loupist's data folder, except its Skills, which workspace discovery
/// loads like any Skill folder.
fn loupist_private(target: &Path, roots: &NativeRoots) -> bool {
    under(target, &roots.loupist_data) && !under(target, &roots.loupist_data.join("skills"))
}

/// How much third-party text can steer this turn's model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exposure {
    Clean,
    /// This turn raised the untrusted-text tier.
    ThisTurn,
    /// This turn is clean, but an earlier reply that read third-party text is
    /// in the model's context.
    Carried,
    /// This turn read a custom-connection result (the stronger tier).
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AskKind {
    Command,
    FileRead,
    FileWrite,
    Instructions,
}

impl AskKind {
    pub const COUNT: usize = 4;

    pub fn index(self) -> usize {
        match self {
            Self::Command => 0,
            Self::FileRead => 1,
            Self::FileWrite => 2,
            Self::Instructions => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandScope {
    /// Third-party text arrived this turn: a blanket answer covers this reply.
    Reply,
    /// Only earlier replies carry it: a blanket answer covers this Chat.
    Chat,
}

/// What an Allow answer records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Grant {
    Command {
        command: String,
        scope: CommandScope,
    },
    Read(GrantedPath),
    Write(GrantedPath),
    InstructionFile(PathBuf),
}

/// A folder (this reply) or one exact file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantedPath {
    pub path: PathBuf,
    pub tree: bool,
}

impl GrantedPath {
    fn covers(&self, target: &Path) -> bool {
        target == self.path || (self.tree && target.starts_with(&self.path))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeAsk {
    pub kind: AskKind,
    pub tool: NativeTool,
    /// The display line: a command's first line or the target path.
    pub target: String,
    /// The full command, bounded.
    pub detail: Option<String>,
    pub grant: Grant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeRefusal {
    /// The app's private data.
    AppPrivate,
    /// A search rooted above the app data root would descend into it.
    SearchCoversAppData,
    /// The target's symlink chain loops or cannot be read.
    Unresolvable,
    /// The user turned this kind down earlier in the reply.
    Declined(AskKind),
    /// Bash under the custom-connection tier.
    OutboundPaused,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeDecision {
    Allow {
        /// The canonical target, for the extension's write pin.
        resolved: Option<PathBuf>,
    },
    Ask(NativeAsk),
    Refuse(NativeRefusal),
}

/// What the user allowed or turned down in this reply. Chat-scope command
/// grants live in the app-session registry, keyed by Loupist and Chat.
#[derive(Default, Debug)]
pub struct NativeGrants {
    commands_for_reply: bool,
    commands: BTreeSet<String>,
    reads: Vec<GrantedPath>,
    writes: Vec<GrantedPath>,
    instruction_files: BTreeSet<PathBuf>,
    declined: BTreeSet<AskKind>,
}

impl NativeGrants {
    /// Records an Allow answer. An Always answer on a Chat-scope command keeps
    /// only that command here: the caller stores the Chat grant in its registry.
    pub fn grant(&mut self, ask: &NativeAsk, decision: ApprovalDecision) {
        match (&ask.grant, decision) {
            (_, ApprovalDecision::Deny) => {}
            (Grant::Command { command, scope }, ApprovalDecision::Always) => {
                if *scope == CommandScope::Reply {
                    self.commands_for_reply = true;
                }
                self.commands.insert(command.clone());
            }
            (Grant::Command { command, .. }, ApprovalDecision::Once) => {
                self.commands.insert(command.clone());
            }
            (Grant::Read(path), _) => self.reads.push(path.clone()),
            (Grant::Write(path), _) => self.writes.push(path.clone()),
            (Grant::InstructionFile(path), _) => {
                self.instruction_files.insert(path.clone());
            }
        }
    }

    /// Not now, no answer in time, or the run stopped: that kind is not asked
    /// again this reply.
    pub fn decline(&mut self, kind: AskKind) {
        self.declined.insert(kind);
    }
}

/// `chat_commands` is the Chat registry's answer for this Loupist and Chat.
pub fn decide(
    request: &NativeRequest,
    roots: &NativeRoots,
    exposure: Exposure,
    grants: &NativeGrants,
    chat_commands: bool,
) -> NativeDecision {
    if request.tool == NativeTool::Bash {
        return decide_command(request, exposure, grants, chat_commands);
    }
    let Some(target) = request.target.as_deref() else {
        return NativeDecision::Refuse(NativeRefusal::AppPrivate);
    };
    let Classified { class, path } = classify(target, roots);
    if class == PathClass::Unresolvable {
        return NativeDecision::Refuse(NativeRefusal::Unresolvable);
    }
    let tool = request.tool;
    let exposed = exposure != Exposure::Clean;
    let allow = NativeDecision::Allow {
        resolved: Some(path.clone()),
    };
    if tool.searches() && class != PathClass::Scratch && class != PathClass::Attachments {
        if class == PathClass::AppPrivate {
            return NativeDecision::Refuse(NativeRefusal::AppPrivate);
        }
        if under(&roots.app_data, &path) {
            return NativeDecision::Refuse(NativeRefusal::SearchCoversAppData);
        }
    }
    let ask = |kind: AskKind, grant: Grant| {
        if grants.declined.contains(&kind) {
            return NativeDecision::Refuse(NativeRefusal::Declined(kind));
        }
        NativeDecision::Ask(NativeAsk {
            kind,
            tool,
            target: display(&path),
            detail: None,
            grant,
        })
    };
    let exact = GrantedPath {
        path: path.clone(),
        tree: false,
    };
    let folder = GrantedPath {
        path: if path.is_dir() {
            path.clone()
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or_default()
        },
        tree: true,
    };
    let granted = |paths: &[GrantedPath]| paths.iter().any(|grant| grant.covers(&path));
    // A folder allowed earlier never covers a credential or startup file in it.
    let granted_exactly =
        |paths: &[GrantedPath]| paths.iter().any(|grant| !grant.tree && grant.path == path);
    if tool.writes() {
        match class {
            PathClass::AppPrivate | PathClass::Attachments => {
                NativeDecision::Refuse(NativeRefusal::AppPrivate)
            }
            PathClass::Scratch | PathClass::Workspace => allow,
            PathClass::Sensitive if granted_exactly(&grants.writes) => allow,
            PathClass::Sensitive => ask(AskKind::FileWrite, Grant::Write(exact)),
            PathClass::Instructions if !exposed || grants.instruction_files.contains(&path) => {
                allow
            }
            PathClass::Instructions => {
                ask(AskKind::Instructions, Grant::InstructionFile(path.clone()))
            }
            PathClass::Outside if !exposed || granted(&grants.writes) => allow,
            PathClass::Outside => ask(AskKind::FileWrite, Grant::Write(folder)),
            PathClass::Unresolvable => unreachable!("refused above"),
        }
    } else {
        match class {
            PathClass::AppPrivate => NativeDecision::Refuse(NativeRefusal::AppPrivate),
            PathClass::Instructions
            | PathClass::Scratch
            | PathClass::Attachments
            | PathClass::Workspace => allow,
            PathClass::Sensitive if granted_exactly(&grants.reads) => allow,
            PathClass::Sensitive => ask(AskKind::FileRead, Grant::Read(exact)),
            PathClass::Outside if !exposed || granted(&grants.reads) => allow,
            PathClass::Outside => ask(AskKind::FileRead, Grant::Read(folder)),
            PathClass::Unresolvable => unreachable!("refused above"),
        }
    }
}

/// The `bash` rows of the table; no folder is involved.
pub fn decide_command(
    request: &NativeRequest,
    exposure: Exposure,
    grants: &NativeGrants,
    chat_commands: bool,
) -> NativeDecision {
    let command = request.command.as_deref().unwrap_or_default();
    let scope = match exposure {
        Exposure::Clean => return NativeDecision::Allow { resolved: None },
        Exposure::Custom => return NativeDecision::Refuse(NativeRefusal::OutboundPaused),
        Exposure::ThisTurn => CommandScope::Reply,
        Exposure::Carried => CommandScope::Chat,
    };
    if grants.commands_for_reply
        || grants.commands.contains(command)
        || (scope == CommandScope::Chat && chat_commands)
    {
        return NativeDecision::Allow { resolved: None };
    }
    if grants.declined.contains(&AskKind::Command) {
        return NativeDecision::Refuse(NativeRefusal::Declined(AskKind::Command));
    }
    NativeDecision::Ask(NativeAsk {
        kind: AskKind::Command,
        tool: NativeTool::Bash,
        target: bounded_chars(command.lines().next().unwrap_or_default(), MAX_TARGET_CHARS),
        detail: Some(command_detail(command)),
        grant: Grant::Command {
            command: command.to_owned(),
            scope,
        },
    })
}

fn display(path: &Path) -> String {
    let text = path.to_string_lossy();
    if text.chars().count() <= MAX_TARGET_CHARS {
        return text.into_owned();
    }
    let tail: String = text
        .chars()
        .rev()
        .take(MAX_TARGET_CHARS - 1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

fn bounded_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let head: String = text.chars().take(max - 1).collect();
    format!("{head}…")
}

/// The command as the card shows it. A command longer than the card holds
/// says how much is hidden, so the user never allows a tail they cannot see
/// without being told.
fn command_detail(command: &str) -> String {
    if command.len() <= MAX_DETAIL_BYTES {
        return command.to_owned();
    }
    let mut end = MAX_DETAIL_BYTES - 64;
    while !command.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n… [+{} bytes not shown]",
        &command[..end],
        command.len() - end
    )
}

const ROOT_INSTRUCTIONS: &[&str] = &["agents.override.md", "agents.md", "claude.md"];

/// Matches what workspace discovery loads: root instructions,
/// `.loupist/AGENTS.md`, `.loupist/config.toml`, Skill entries (`skills/*.md`,
/// `skills/**/SKILL.md`) under `.loupist`, `.agents` and `.pi`, and the global
/// `AGENTS.md` and everything under the global `skills` folder. Names compare
/// without case, on canonical paths, so an alias cannot reach them.
fn instruction_file(target: &Path, roots: &NativeRoots) -> bool {
    let relative = |root: &Path| {
        target.strip_prefix(root).ok().map(|rest| {
            rest.components()
                .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
        })
    };
    let skill_entry = |rest: &[String]| match rest {
        [file] => file.ends_with(".md"),
        [.., file] => file == "skill.md",
        [] => false,
    };
    let workspace = relative(&roots.workspace).is_some_and(|parts| match parts.as_slice() {
        [file] => ROOT_INSTRUCTIONS.contains(&file.as_str()),
        [folder, file] if folder == ".loupist" => file == "agents.md" || file == "config.toml",
        [folder, skills, rest @ ..] => {
            matches!(folder.as_str(), ".loupist" | ".agents" | ".pi")
                && skills == "skills"
                && skill_entry(rest)
        }
        _ => false,
    });
    workspace
        || relative(&roots.global).is_some_and(|parts| match parts.as_slice() {
            [file] => file == "agents.md",
            [skills, ..] => skills == "skills",
            _ => false,
        })
}

#[derive(Clone, Copy)]
enum Reach {
    File,
    Tree,
}

/// Credential stores, cloud and container configs, keychains, launch agents,
/// shell startup files and git config, relative to the home directory.
const SENSITIVE_HOME: &[(&str, Reach)] = &[
    (".ssh", Reach::Tree),
    (".gnupg", Reach::Tree),
    (".aws", Reach::Tree),
    (".config/gcloud", Reach::Tree),
    (".kube", Reach::Tree),
    (".docker/config.json", Reach::File),
    (".netrc", Reach::File),
    (".git-credentials", Reach::File),
    (".npmrc", Reach::File),
    (".pypirc", Reach::File),
    ("Library/Keychains", Reach::Tree),
    ("Library/LaunchAgents", Reach::Tree),
    (".zshrc", Reach::File),
    (".zprofile", Reach::File),
    (".zshenv", Reach::File),
    (".zlogin", Reach::File),
    (".bashrc", Reach::File),
    (".bash_profile", Reach::File),
    (".profile", Reach::File),
    (".config/fish", Reach::Tree),
    (".gitconfig", Reach::File),
];

/// The home table above, plus any `.git/hooks/**` or `.git/config`.
fn sensitive(target: &Path, home: Option<&Path>) -> bool {
    let parts: Vec<String> = target
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    let git = parts.windows(2).enumerate().any(|(at, pair)| {
        pair[0] == ".git" && (pair[1] == "hooks" || (pair[1] == "config" && at + 2 == parts.len()))
    });
    if git {
        return true;
    }
    let Some(home) = home else {
        return false;
    };
    let Some(rest) = lower_relative(target, home) else {
        return false;
    };
    SENSITIVE_HOME.iter().any(|(entry, reach)| {
        let entry: Vec<String> = entry.split('/').map(str::to_lowercase).collect();
        match reach {
            Reach::File => rest == entry,
            Reach::Tree => rest.len() >= entry.len() && rest[..entry.len()] == entry[..],
        }
    })
}

/// `target`'s components under `root`, lowercased, when `root` is an ancestor
/// by identity or by case-insensitive prefix.
fn lower_relative(target: &Path, root: &Path) -> Option<Vec<String>> {
    if !under(target, root) {
        return None;
    }
    let root_depth = root.components().count();
    Some(
        target
            .components()
            .skip(root_depth)
            .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
            .collect(),
    )
}

/// Root containment by filesystem identity (`(dev, ino)` of each existing
/// ancestor, like `fs/pinned_root.rs`), then by case-insensitive prefix, so a
/// firmlink or case alias of a root still counts as inside it.
fn under(target: &Path, root: &Path) -> bool {
    if root.as_os_str().is_empty() {
        return false;
    }
    if case_insensitive_prefix(target, root) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let identity = |path: &Path| {
            std::fs::metadata(path)
                .ok()
                .map(|metadata| (metadata.dev(), metadata.ino()))
        };
        if let Some(root_identity) = identity(root) {
            return target
                .ancestors()
                .any(|ancestor| identity(ancestor) == Some(root_identity));
        }
    }
    false
}

fn case_insensitive_prefix(target: &Path, root: &Path) -> bool {
    let lower = |path: &Path| {
        path.components()
            .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
            .collect::<Vec<_>>()
    };
    let (target, root) = (lower(target), lower(root));
    target.len() >= root.len() && target[..root.len()] == root[..]
}

/// A root's resolved form; a root that cannot be resolved keeps its lexical form.
fn resolved(path: &Path) -> PathBuf {
    resolve_links(path).unwrap_or_else(|| lexical(path))
}

/// Symlink hops a target may take before it counts as a loop.
const MAX_LINK_HOPS: usize = 40;

/// Lexically normalized, with the longest existing ancestor canonicalized. A
/// dangling symlink is followed to where it points, since that is where a
/// write through it lands. None for a link chain that loops or cannot be read.
pub fn resolve_links(path: &Path) -> Option<PathBuf> {
    let mut normal = lexical(path);
    for _ in 0..MAX_LINK_HOPS {
        let mut existing = normal.as_path();
        let mut rest: Vec<std::ffi::OsString> = Vec::new();
        loop {
            if let Ok(canonical) = existing.canonicalize() {
                return Some(rest.iter().rev().fold(plain_path(canonical), |path, part| {
                    if part == ".." {
                        path.parent().map(Path::to_path_buf).unwrap_or(path)
                    } else {
                        path.join(part)
                    }
                }));
            }
            if existing
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                let target = std::fs::read_link(existing).ok()?;
                let parent = existing.parent()?.canonicalize().ok()?;
                // `..` inside the link's target stays for the fold above, which
                // applies it after the symlinks before it are resolved.
                normal = rest
                    .iter()
                    .rev()
                    .fold(parent.join(target), |path, part| path.join(part));
                break;
            }
            match (existing.parent(), existing.file_name()) {
                (Some(parent), Some(name)) => {
                    rest.push(name.to_owned());
                    existing = parent;
                }
                _ => return Some(normal),
            }
        }
    }
    None
}

/// `.` and `..` removed without touching the disk.
fn lexical(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    normal
}

/// Windows canonical paths carry a verbatim prefix the extension's realpath
/// does not; both sides compare plain paths.
fn plain_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture {
        _temporary: tempfile::TempDir,
        roots: NativeRoots,
        home: PathBuf,
        other_chat_temp: PathBuf,
    }

    fn fixture() -> Fixture {
        let temporary = tempfile::tempdir().unwrap();
        let folder = |path: PathBuf| {
            std::fs::create_dir_all(&path).unwrap();
            path.canonicalize().unwrap()
        };
        let home = folder(temporary.path().join("home"));
        let app_data = folder(home.join("Library/Application Support/Loupist"));
        let workspace = folder(home.join("Research"));
        let loupist_data = folder(workspace.join(".loupist"));
        let runtime = app_data.join("loupists/loupist-a/pi-runtime");
        let scratch = folder(runtime.join("chat-a/temp"));
        let other_chat_temp = folder(runtime.join("chat-b/temp"));
        let attachments = folder(loupist_data.join("loupists/loupist-a/attachments/chat-a"));
        for path in [
            "loupist.sqlite3",
            "loupists/loupist-a/attachments/chat-b/brief.pdf.md",
            "loupists/loupist-a/activity/run-1.jsonl",
        ] {
            let file = loupist_data.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "x").unwrap();
        }
        for path in [
            "pi/auth.json",
            "pi/models.json",
            "custom-connections/connections.json",
            "skills/personal/SKILL.md",
            "skills/personal/helper.py",
            "AGENTS.md",
            "loupists/loupist-a/pi-runtime/chat-a/.pi/settings.json",
        ] {
            let file = app_data.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "x").unwrap();
        }
        for path in [
            "AGENTS.md",
            ".loupist/AGENTS.md",
            ".loupist/config.toml",
            ".loupist/skills/screen/SKILL.md",
            ".loupist/skills/screen/screen.py",
            "notes/plan.md",
            ".git/config",
            ".git/hooks/pre-commit",
        ] {
            let file = workspace.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "x").unwrap();
        }
        for path in [
            ".ssh/id_ed25519",
            ".zshrc",
            ".config/gcloud/credentials.db",
            "Documents/shared.csv",
        ] {
            let file = home.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "x").unwrap();
        }
        std::fs::write(attachments.join("brief.pdf.md"), "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(app_data.join("pi"), scratch.join("link-to-pi")).unwrap();
            // Dangling links: their targets do not exist.
            symlink(home.join(".zshenv"), workspace.join("report.md")).unwrap();
            symlink("../.zshenv", workspace.join("relative.md")).unwrap();
            symlink("report.md", workspace.join("chain.md")).unwrap();
            symlink(app_data.join("pi/secret.json"), workspace.join("db.md")).unwrap();
            symlink("loop.md", workspace.join("loop.md")).unwrap();
        }
        let roots = NativeRoots::new(
            &workspace,
            &app_data,
            &app_data,
            &loupist_data,
            &scratch,
            &attachments,
            Some(&home),
        );
        Fixture {
            _temporary: temporary,
            roots,
            home,
            other_chat_temp,
        }
    }

    fn request(tool: &str, path: impl AsRef<Path>) -> NativeRequest {
        NativeRequest::parse(&json!({"name": tool, "path": path.as_ref().to_string_lossy()}))
            .unwrap()
    }

    fn bash(command: &str) -> NativeRequest {
        NativeRequest::parse(&json!({"name": "bash", "command": command})).unwrap()
    }

    fn kind(decision: &NativeDecision) -> &'static str {
        match decision {
            NativeDecision::Allow { .. } => "allow",
            NativeDecision::Ask(ask) => match ask.kind {
                AskKind::Command => "ask command",
                AskKind::FileRead => "ask file_read",
                AskKind::FileWrite => "ask file_write",
                AskKind::Instructions => "ask instructions",
            },
            NativeDecision::Refuse(NativeRefusal::AppPrivate) => "refuse app_private",
            NativeDecision::Refuse(NativeRefusal::SearchCoversAppData) => "refuse search",
            NativeDecision::Refuse(NativeRefusal::Unresolvable) => "refuse unresolvable",
            NativeDecision::Refuse(NativeRefusal::Declined(_)) => "refuse declined",
            NativeDecision::Refuse(NativeRefusal::OutboundPaused) => "refuse outbound",
        }
    }

    #[test]
    fn paths_classify_by_canonical_location_in_table_order() {
        let f = fixture();
        let home = &f.home;
        let app = &f.roots.app_data;
        let ws = &f.roots.workspace;
        let cases: Vec<(PathBuf, PathClass)> = vec![
            (ws.join("AGENTS.md"), PathClass::Instructions),
            (ws.join("notes/../agents.MD"), PathClass::Instructions),
            (ws.join(".loupist/config.toml"), PathClass::Instructions),
            (
                ws.join(".loupist/skills/screen/SKILL.md"),
                PathClass::Instructions,
            ),
            (
                ws.join(".loupist/skills/screen/screen.py"),
                PathClass::Workspace,
            ),
            (app.join("AGENTS.md"), PathClass::Instructions),
            (
                app.join("skills/personal/helper.py"),
                PathClass::Instructions,
            ),
            (f.roots.scratch.join("deck.pptx"), PathClass::Scratch),
            (
                f.roots.attachments.join("brief.pdf.md"),
                PathClass::Attachments,
            ),
            (f.other_chat_temp.join("note.txt"), PathClass::AppPrivate),
            (
                app.join("loupists/loupist-a/pi-runtime/chat-a/.pi/settings.json"),
                PathClass::AppPrivate,
            ),
            (app.join("pi/auth.json"), PathClass::AppPrivate),
            (app.join("pi/models.json"), PathClass::AppPrivate),
            (
                app.join("custom-connections/connections.json"),
                PathClass::AppPrivate,
            ),
            (
                home.join("library/application support/LOUPIST/pi/auth.json"),
                PathClass::AppPrivate,
            ),
            (
                ws.join("../Library/Application Support/Loupist/pi/auth.json"),
                PathClass::AppPrivate,
            ),
            (ws.join(".loupist/loupist.sqlite3"), PathClass::AppPrivate),
            (ws.join(".LOUPIST/loupist.sqlite3-wal"), PathClass::AppPrivate),
            (
                ws.join(".loupist/loupists/loupist-a/attachments/chat-b/brief.pdf.md"),
                PathClass::AppPrivate,
            ),
            (
                ws.join(".loupist/loupists/loupist-a/activity/run-1.jsonl"),
                PathClass::AppPrivate,
            ),
            (home.join(".ssh/id_ed25519"), PathClass::Sensitive),
            (home.join(".SSH/known_hosts"), PathClass::Sensitive),
            (home.join(".zshrc"), PathClass::Sensitive),
            (
                home.join(".config/gcloud/credentials.db"),
                PathClass::Sensitive,
            ),
            (
                home.join("Library/LaunchAgents/com.example.plist"),
                PathClass::Sensitive,
            ),
            (home.join(".gitconfig"), PathClass::Sensitive),
            (home.join(".zshrc.backup"), PathClass::Outside),
            (ws.join(".git/hooks/pre-commit"), PathClass::Sensitive),
            (ws.join(".git/config"), PathClass::Sensitive),
            (ws.join(".git/config.lock"), PathClass::Workspace),
            (ws.join("notes/plan.md"), PathClass::Workspace),
            (ws.join("new/report.md"), PathClass::Workspace),
            (PathBuf::from("relative/report.md"), PathClass::Workspace),
            (home.join("Documents/shared.csv"), PathClass::Outside),
        ];
        for (path, expected) in cases {
            assert_eq!(
                classify(&path, &f.roots).class,
                expected,
                "{}",
                path.display()
            );
        }
        #[cfg(unix)]
        assert_eq!(
            classify(&f.roots.scratch.join("link-to-pi/auth.json"), &f.roots).class,
            PathClass::AppPrivate,
            "a symlink inside scratch resolves to its target"
        );
    }

    /// A write through a dangling link lands where the link points, so the
    /// decision is about that target, and the canonical path the extension
    /// pins the write to is the target too.
    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_is_decided_by_where_it_points() {
        let f = fixture();
        let ws = &f.roots.workspace;
        for (link, target, class) in [
            ("report.md", f.home.join(".zshenv"), PathClass::Sensitive),
            ("relative.md", f.home.join(".zshenv"), PathClass::Sensitive),
            ("chain.md", f.home.join(".zshenv"), PathClass::Sensitive),
            (
                "db.md",
                f.roots.app_data.join("pi/secret.json"),
                PathClass::AppPrivate,
            ),
        ] {
            let classified = classify(&ws.join(link), &f.roots);
            assert_eq!(classified.class, class, "{link}");
            assert_eq!(classified.path, target, "{link}");
        }
        for exposure in [Exposure::Clean, Exposure::ThisTurn] {
            assert_eq!(
                kind(&decide(
                    &request("write", ws.join("report.md")),
                    &f.roots,
                    exposure,
                    &NativeGrants::default(),
                    false
                )),
                "ask file_write"
            );
            assert_eq!(
                kind(&decide(
                    &request("write", ws.join("db.md")),
                    &f.roots,
                    exposure,
                    &NativeGrants::default(),
                    false
                )),
                "refuse app_private"
            );
            for tool in ["read", "write", "ls"] {
                assert_eq!(
                    kind(&decide(
                        &request(tool, ws.join("loop.md")),
                        &f.roots,
                        exposure,
                        &NativeGrants::default(),
                        false
                    )),
                    "refuse unresolvable",
                    "{tool}"
                );
            }
        }
        // A link's target with `..` after a symlink applies the `..` to the
        // resolved link, as the OS does.
        std::os::unix::fs::symlink(f.home.join("Documents"), ws.join("docs-link")).unwrap();
        std::os::unix::fs::symlink("docs-link/../.zshenv", ws.join("via-docs.md")).unwrap();
        assert_eq!(
            classify(&ws.join("via-docs.md"), &f.roots).path,
            f.home.join(".zshenv")
        );
    }

    #[test]
    fn the_policy_table_decides_each_tool_class_and_exposure() {
        let f = fixture();
        let home = &f.home;
        let app = &f.roots.app_data;
        let ws = &f.roots.workspace;
        let clean = NativeGrants::default();
        let rows: Vec<(NativeRequest, &str, &str)> = vec![
            (
                request("read", app.join("pi/auth.json")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("ls", app.join("pi")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("read", ws.join(".loupist/loupist.sqlite3")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("write", ws.join(".loupist/loupist.sqlite3")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("grep", ws.join(".loupist")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("read", ws.join(".loupist/skills/screen/screen.py")),
                "allow",
                "allow",
            ),
            (
                request("read", home.join(".ssh/id_ed25519")),
                "ask file_read",
                "ask file_read",
            ),
            (
                request("ls", home.join(".ssh")),
                "ask file_read",
                "ask file_read",
            ),
            (request("read", ws.join("AGENTS.md")), "allow", "allow"),
            (
                request("read", f.roots.scratch.join("deck.pptx")),
                "allow",
                "allow",
            ),
            (
                request("read", f.roots.attachments.join("brief.pdf.md")),
                "allow",
                "allow",
            ),
            (request("read", ws.join("notes/plan.md")), "allow", "allow"),
            (
                request("read", home.join("Documents/shared.csv")),
                "allow",
                "ask file_read",
            ),
            (
                request("grep", app.join("pi")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("find", home.clone()),
                "refuse search",
                "refuse search",
            ),
            (
                request("grep", home.join("..")),
                "refuse search",
                "refuse search",
            ),
            (
                request("grep", home.join(".ssh")),
                "ask file_read",
                "ask file_read",
            ),
            (request("grep", ws.clone()), "allow", "allow"),
            (request("grep", f.roots.scratch.clone()), "allow", "allow"),
            (
                request("find", home.join("Documents")),
                "allow",
                "ask file_read",
            ),
            (
                request("write", app.join("pi/models.json")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("edit", app.join("custom-connections/connections.json")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("write", f.roots.attachments.join("brief.pdf.md")),
                "refuse app_private",
                "refuse app_private",
            ),
            (
                request("write", home.join(".zshrc")),
                "ask file_write",
                "ask file_write",
            ),
            (
                request("edit", ws.join(".git/hooks/pre-commit")),
                "ask file_write",
                "ask file_write",
            ),
            (
                request("write", f.roots.scratch.join("out.csv")),
                "allow",
                "allow",
            ),
            (request("write", ws.join("notes/plan.md")), "allow", "allow"),
            (
                request("write", ws.join("AGENTS.md")),
                "allow",
                "ask instructions",
            ),
            (
                request("edit", app.join("skills/personal/SKILL.md")),
                "allow",
                "ask instructions",
            ),
            (
                request("write", ws.join(".loupist/config.toml")),
                "allow",
                "ask instructions",
            ),
            (
                request("write", home.join("Documents/out.csv")),
                "allow",
                "ask file_write",
            ),
            (bash("ls"), "allow", "ask command"),
        ];
        for (request, clean_expected, exposed_expected) in &rows {
            assert_eq!(
                kind(&decide(request, &f.roots, Exposure::Clean, &clean, false)),
                *clean_expected,
                "clean {request:?}"
            );
            for exposure in [Exposure::ThisTurn, Exposure::Carried] {
                let decision = decide(request, &f.roots, exposure, &clean, false);
                assert_eq!(
                    kind(&decision),
                    *exposed_expected,
                    "{exposure:?} {request:?}"
                );
                if let NativeDecision::Ask(ask) = &decision {
                    assert_eq!(ask.tool, request.tool);
                    assert!(!ask.target.is_empty());
                }
            }
        }
        let custom = decide(&bash("ls"), &f.roots, Exposure::Custom, &clean, false);
        assert_eq!(kind(&custom), "refuse outbound");
        let custom = decide(
            &request("read", home.join("Documents/shared.csv")),
            &f.roots,
            Exposure::Custom,
            &clean,
            false,
        );
        assert_eq!(kind(&custom), "ask file_read");
    }

    #[test]
    fn allowed_writes_return_the_canonical_target() {
        let f = fixture();
        let decision = decide(
            &request("write", f.roots.workspace.join("notes/./new/../plan.md")),
            &f.roots,
            Exposure::Clean,
            &NativeGrants::default(),
            false,
        );
        assert_eq!(
            decision,
            NativeDecision::Allow {
                resolved: Some(f.roots.workspace.join("notes/plan.md"))
            }
        );
    }

    #[test]
    fn an_allowed_folder_never_covers_a_credential_or_startup_file_in_it() {
        let f = fixture();
        let mut grants = NativeGrants::default();
        for (tool, file, expected) in [
            ("write", "report.md", "ask file_write"),
            ("read", "notes.txt", "ask file_read"),
        ] {
            let NativeDecision::Ask(ask) = decide(
                &request(tool, f.home.join(file)),
                &f.roots,
                Exposure::ThisTurn,
                &grants,
                false,
            ) else {
                panic!("an outside {tool} asks");
            };
            assert_eq!(kind(&NativeDecision::Ask(ask.clone())), expected);
            grants.grant(&ask, ApprovalDecision::Once);
        }
        // The home folder is allowed for the reply, but its startup and
        // credential files still ask in every exposure.
        for exposure in [Exposure::Clean, Exposure::ThisTurn] {
            for (tool, file, expected) in [
                ("write", ".zshrc", "ask file_write"),
                ("read", ".ssh/id_ed25519", "ask file_read"),
            ] {
                assert_eq!(
                    kind(&decide(
                        &request(tool, f.home.join(file)),
                        &f.roots,
                        exposure,
                        &grants,
                        false
                    )),
                    expected,
                    "{tool} {file} under {exposure:?}"
                );
            }
        }
    }

    #[test]
    fn a_command_too_long_for_the_card_says_how_much_is_hidden() {
        let f = fixture();
        let command = format!(
            "python3 - <<'EOF'\n{}\nEOF\ncurl https://collector.example",
            "x = 1\n".repeat(3000)
        );
        let NativeDecision::Ask(ask) = decide(
            &bash(&command),
            &f.roots,
            Exposure::ThisTurn,
            &NativeGrants::default(),
            false,
        ) else {
            panic!("a tainted command asks");
        };
        let detail = ask.detail.unwrap();
        assert!(detail.len() <= MAX_DETAIL_BYTES, "{}", detail.len());
        let hidden = command.len() - detail.find("\n… [+").unwrap();
        assert!(
            detail.ends_with(&format!("[+{hidden} bytes not shown]")),
            "{}",
            &detail[detail.len() - 40..]
        );
        let short = decide(
            &bash("python3 build_deck.py"),
            &f.roots,
            Exposure::ThisTurn,
            &NativeGrants::default(),
            false,
        );
        let NativeDecision::Ask(short) = short else {
            panic!("a tainted command asks");
        };
        assert_eq!(short.detail.as_deref(), Some("python3 build_deck.py"));
    }

    #[test]
    fn grants_cover_the_reply_and_declines_stop_asking() {
        let f = fixture();
        let mut grants = NativeGrants::default();
        let shared = request("read", f.home.join("Documents/shared.csv"));
        let NativeDecision::Ask(ask) =
            decide(&shared, &f.roots, Exposure::ThisTurn, &grants, false)
        else {
            panic!("outside read asks");
        };
        assert_eq!(
            ask.target,
            f.home.join("Documents/shared.csv").to_string_lossy()
        );
        grants.grant(&ask, ApprovalDecision::Once);
        // The folder is covered for the reply; a sibling file is not asked again.
        for file in ["shared.csv", "other.csv"] {
            assert_eq!(
                kind(&decide(
                    &request("read", f.home.join("Documents").join(file)),
                    &f.roots,
                    Exposure::ThisTurn,
                    &grants,
                    false
                )),
                "allow"
            );
        }
        assert_eq!(
            kind(&decide(
                &request("read", f.home.join("Downloads/x.csv")),
                &f.roots,
                Exposure::ThisTurn,
                &grants,
                false
            )),
            "ask file_read"
        );
        // A sensitive grant covers that one file only.
        let NativeDecision::Ask(key) = decide(
            &request("read", f.home.join(".ssh/id_ed25519")),
            &f.roots,
            Exposure::Clean,
            &grants,
            false,
        ) else {
            panic!("sensitive read asks");
        };
        grants.grant(&key, ApprovalDecision::Once);
        assert_eq!(
            kind(&decide(
                &request("read", f.home.join(".ssh/id_ed25519")),
                &f.roots,
                Exposure::Clean,
                &grants,
                false
            )),
            "allow"
        );
        assert_eq!(
            kind(&decide(
                &request("read", f.home.join(".ssh/known_hosts")),
                &f.roots,
                Exposure::Clean,
                &grants,
                false
            )),
            "ask file_read"
        );

        grants.decline(AskKind::FileWrite);
        assert_eq!(
            decide(
                &request("write", f.home.join("Documents/out.csv")),
                &f.roots,
                Exposure::ThisTurn,
                &grants,
                false
            ),
            NativeDecision::Refuse(NativeRefusal::Declined(AskKind::FileWrite))
        );
        assert_eq!(
            kind(&decide(
                &request("write", f.roots.workspace.join("out.csv")),
                &f.roots,
                Exposure::ThisTurn,
                &grants,
                false
            )),
            "allow"
        );
    }

    #[test]
    fn command_grants_follow_the_exposure_scope() {
        let f = fixture();
        let mut grants = NativeGrants::default();
        let once = bash("python3 build_deck.py");
        let NativeDecision::Ask(ask) = decide(&once, &f.roots, Exposure::ThisTurn, &grants, false)
        else {
            panic!("tainted bash asks");
        };
        assert_eq!(ask.target, "python3 build_deck.py");
        assert_eq!(ask.detail.as_deref(), Some("python3 build_deck.py"));
        assert_eq!(
            ask.grant,
            Grant::Command {
                command: "python3 build_deck.py".into(),
                scope: CommandScope::Reply
            }
        );
        grants.grant(&ask, ApprovalDecision::Once);
        assert_eq!(
            kind(&decide(&once, &f.roots, Exposure::ThisTurn, &grants, false)),
            "allow"
        );
        assert_eq!(
            kind(&decide(
                &bash("python3 build_deck.py --again"),
                &f.roots,
                Exposure::ThisTurn,
                &grants,
                false
            )),
            "ask command"
        );

        // Always under ThisTurn covers the reply.
        let NativeDecision::Ask(ask) =
            decide(&bash("ls"), &f.roots, Exposure::ThisTurn, &grants, false)
        else {
            panic!();
        };
        grants.grant(&ask, ApprovalDecision::Always);
        assert_eq!(
            kind(&decide(
                &bash("whoami"),
                &f.roots,
                Exposure::ThisTurn,
                &grants,
                false
            )),
            "allow"
        );

        // Carried only: Always is a Chat grant the caller stores; the Chat
        // grant never applies once this turn reads third-party text.
        let mut carried = NativeGrants::default();
        let NativeDecision::Ask(ask) =
            decide(&bash("ls"), &f.roots, Exposure::Carried, &carried, false)
        else {
            panic!();
        };
        assert_eq!(
            ask.grant,
            Grant::Command {
                command: "ls".into(),
                scope: CommandScope::Chat
            }
        );
        carried.grant(&ask, ApprovalDecision::Always);
        assert_eq!(
            kind(&decide(
                &bash("whoami"),
                &f.roots,
                Exposure::Carried,
                &carried,
                false
            )),
            "ask command"
        );
        assert_eq!(
            kind(&decide(
                &bash("whoami"),
                &f.roots,
                Exposure::Carried,
                &carried,
                true
            )),
            "allow"
        );
        assert_eq!(
            kind(&decide(
                &bash("whoami"),
                &f.roots,
                Exposure::ThisTurn,
                &carried,
                true
            )),
            "ask command"
        );

        // The allowed command itself stays allowed; after Not now, others are
        // refused without a card, and a clean turn never asks.
        carried.decline(AskKind::Command);
        assert_eq!(
            kind(&decide(
                &bash("ls"),
                &f.roots,
                Exposure::Carried,
                &carried,
                false
            )),
            "allow"
        );
        assert_eq!(
            decide(&bash("id"), &f.roots, Exposure::Carried, &carried, false),
            NativeDecision::Refuse(NativeRefusal::Declined(AskKind::Command))
        );
        assert_eq!(
            kind(&decide(
                &bash("id"),
                &f.roots,
                Exposure::Clean,
                &carried,
                false
            )),
            "allow"
        );
    }

    #[test]
    fn a_long_command_shows_its_first_line_and_a_bounded_body() {
        let f = fixture();
        let body = format!("echo start\n{}", "x".repeat(MAX_DETAIL_BYTES * 2));
        let NativeDecision::Ask(ask) = decide(
            &bash(&body),
            &f.roots,
            Exposure::ThisTurn,
            &NativeGrants::default(),
            false,
        ) else {
            panic!();
        };
        assert_eq!(ask.target, "echo start");
        let detail = ask.detail.unwrap();
        assert!(detail.len() <= MAX_DETAIL_BYTES);
        assert!(detail.ends_with(" bytes not shown]"), "{detail:.40}");
    }

    #[test]
    fn requests_are_parsed_strictly_at_the_boundary() {
        assert_eq!(
            NativeRequest::parse(&json!({"name":"read","path":"/tmp/a"})),
            Some(NativeRequest {
                tool: NativeTool::Read,
                target: Some("/tmp/a".into()),
                command: None
            })
        );
        assert_eq!(
            NativeRequest::parse(&json!({"name":"bash","command":"ls"})),
            Some(NativeRequest {
                tool: NativeTool::Bash,
                target: None,
                command: Some("ls".into())
            })
        );
        for malformed in [
            json!({"name":"browser","path":"/tmp/a"}),
            json!({"name":"read"}),
            json!({"name":"bash"}),
            json!({"name":"bash","command":""}),
            json!({"name":"read","path":"/tmp/a\u{0}"}),
            json!({"name":"read","path":"/".repeat(MAX_PATH_BYTES + 1)}),
            json!({"name":"bash","command":"x".repeat(MAX_COMMAND_BYTES + 1)}),
            json!({"name":"read","path":"/tmp/a","extra":1}),
            json!({"name":"read","path":7}),
            json!("read"),
        ] {
            assert_eq!(NativeRequest::parse(&malformed), None, "{malformed}");
        }
    }
}
