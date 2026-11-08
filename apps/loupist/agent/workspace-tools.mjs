import {
  createBashToolDefinition,
  createEditToolDefinition,
  createFindToolDefinition,
  createGrepToolDefinition,
  createLsToolDefinition,
  createReadToolDefinition,
  createWriteToolDefinition,
} from "@earendil-works/pi-coding-agent";
import { constants, realpathSync } from "node:fs";
import { access, mkdir, readFile, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const WORKSPACE_ENVIRONMENT_KEYS = new Set([
  "PATH", "HOME", "USER", "LOGNAME", "TMPDIR", "TMP", "TEMP",
  "LANG", "LANGUAGE", "LC_ALL", "LC_CTYPE", "LC_NUMERIC", "LC_TIME",
  "LC_COLLATE", "LC_MONETARY", "LC_MESSAGES", "LC_PAPER", "LC_NAME",
  "LC_ADDRESS", "LC_TELEPHONE", "LC_MEASUREMENT", "LC_IDENTIFICATION", "TZ",
  "TERM", "COLORTERM", "NO_COLOR",
  "OS", "SYSTEMROOT", "SYSTEMDRIVE", "WINDIR", "COMSPEC", "PATHEXT",
  "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "APPDATA", "LOCALAPPDATA",
  "PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMDATA",
]);

/** Keep ordinary host execution settings without forwarding app credentials or shell startup hooks. */
export function allowedWorkspaceEnvironment(env = process.env) {
  return Object.fromEntries(Object.entries(env).filter(([key, value]) =>
    WORKSPACE_ENVIRONMENT_KEYS.has(key.toUpperCase()) && typeof value === "string",
  ));
}

const UNICODE_SPACES = /[  -   　]/gu;

function windowsShellPath(path) {
  if (process.platform !== "win32" || !path.startsWith("/") || path.startsWith("//") || path.includes("\\")) {
    return path;
  }
  const match = path.match(/^\/(?:mnt\/|cygdrive\/)?([a-z])(?:\/(.*))?$/iu);
  return match ? `${match[1].toUpperCase()}:\\${match[2]?.replaceAll("/", "\\") ?? ""}` : path;
}

/** The absolute file a native tool targets, resolved the way Pi's file tools
 * resolve their `path` argument against the workspace. */
export function nativeTargetPath(workspaceRoot, input) {
  if (typeof input !== "string") return undefined;
  let path = windowsShellPath(input.replace(UNICODE_SPACES, " ").replace(/^@/u, ""));
  if (path === "~") path = homedir();
  else if (path.startsWith("~/") || (process.platform === "win32" && path.startsWith("~\\"))) {
    path = join(homedir(), path.slice(2));
  } else if (/^file:\/\//u.test(path)) path = fileURLToPath(path);
  return isAbsolute(path) ? resolve(path) : resolve(workspaceRoot, path);
}

/** The form Rust pins a write to: the longest existing ancestor's real path
 * plus the parts not yet on disk, so a link swapped in later changes it. */
export function canonicalTargetPath(path) {
  const rest = [];
  let existing = resolve(path);
  for (;;) {
    try {
      return join(realpathSync.native(existing), ...rest.reverse());
    } catch {
      const parent = dirname(existing);
      if (parent === existing) return resolve(path);
      rest.push(basename(existing));
      existing = parent;
    }
  }
}

const sameTarget = (left, right) => process.platform === "win32"
  ? left.toLowerCase() === right.toLowerCase() : left === right;

/** File operations that write only the target Rust authorized, re-resolved at write time. */
function authorizedFileOperations(name, expected) {
  const write = async (path, content) => {
    if (!sameTarget(canonicalTargetPath(path), expected)) {
      throw new Error("Native file target changed after authorization");
    }
    await writeFile(path, content, "utf-8");
  };
  return name === "write"
    ? { writeFile: write, mkdir: (directory) => mkdir(directory, { recursive: true }).then(() => {}) }
    : { readFile: (path) => readFile(path), writeFile: write,
      access: (path) => access(path, constants.R_OK | constants.W_OK) };
}

const FILE_WRITERS = { write: createWriteToolDefinition, edit: createEditToolDefinition };
const SEARCHERS = new Set(["ls", "find", "grep"]);

/** What `native.authorize` is told about one call: the resolved file for read,
 * write and edit; the resolved search root for ls, find and grep (the
 * workspace by default, as Pi resolves it); the exact command for bash. */
export function nativeRequest(workspaceRoot, name, input) {
  if (name === "bash") return { command: input?.command };
  if (FILE_WRITERS[name] || name === "read") {
    const path = nativeTargetPath(workspaceRoot, input?.path);
    return path === undefined ? {} : { path };
  }
  if (SEARCHERS.has(name)) return { path: nativeTargetPath(workspaceRoot, input?.path || ".") };
  return {};
}

/** Native Pi tools operate on the selected workspace while Pi itself keeps a private runtime CWD.
 * `authorize(name, request, signal, id)` asks Rust before every execution with the
 * `nativeRequest` above; Rust owns the permission policy, exported-text trust and the
 * canonical target (`resolved`) a write or edit is pinned to. */
export function registerWorkspaceTools(pi, workspaceRoot, authorize) {
  const definitions = [
    createReadToolDefinition(workspaceRoot),
    createWriteToolDefinition(workspaceRoot),
    createEditToolDefinition(workspaceRoot),
    createBashToolDefinition(workspaceRoot, {
      exposeSessionEnvironment: false,
      spawnHook: ({ command, env }) => ({
        command,
        cwd: workspaceRoot,
        env: allowedWorkspaceEnvironment(env),
      }),
    }),
    createLsToolDefinition(workspaceRoot),
    createFindToolDefinition(workspaceRoot),
    createGrepToolDefinition(workspaceRoot),
  ];

  for (const definition of definitions) {
    pi.registerTool({
      ...definition,
      async execute(id, input, signal, onUpdate, ctx) {
        const writer = FILE_WRITERS[definition.name];
        const request = nativeRequest(workspaceRoot, definition.name, input);
        const permission = authorize ? await authorize(definition.name, request, signal, id) : undefined;
        const boundary = permission?.untrusted_text;
        const deliver = (result) => boundary ? {
          ...result,
          content: result.content.map((part) => part.type === "text"
            ? { ...part, text: `${boundary.prefix}${part.text}${boundary.suffix}` } : part),
        } : result;
        const expected = typeof permission?.resolved === "string"
          ? permission.resolved : request.path === undefined ? undefined : canonicalTargetPath(request.path);
        const run = writer && expected !== undefined
          ? writer(workspaceRoot, { operations: authorizedFileOperations(definition.name, expected) })
          : definition;
        const result = await run.execute(id, input, signal,
          onUpdate ? (update) => onUpdate(deliver(update)) : undefined, {
          ...ctx,
          cwd: workspaceRoot,
        });
        return deliver(result);
      },
    });
  }
}
