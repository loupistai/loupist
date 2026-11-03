import {
  createBashToolDefinition,
  createEditToolDefinition,
  createFindToolDefinition,
  createGrepToolDefinition,
  createLsToolDefinition,
  createReadToolDefinition,
  createWriteToolDefinition,
} from "@earendil-works/pi-coding-agent";
import { constants } from "node:fs";
import { access, mkdir, readFile, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
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

/** The absolute file a native write or edit targets, resolved the way Pi's
 * file tools resolve their `path` argument against the workspace. */
export function nativeTargetPath(workspaceRoot, input) {
  if (typeof input !== "string") return undefined;
  let path = windowsShellPath(input.replace(UNICODE_SPACES, " ").replace(/^@/u, ""));
  if (path === "~") path = homedir();
  else if (path.startsWith("~/") || (process.platform === "win32" && path.startsWith("~\\"))) {
    path = join(homedir(), path.slice(2));
  } else if (/^file:\/\//u.test(path)) path = fileURLToPath(path);
  return isAbsolute(path) ? resolve(path) : resolve(workspaceRoot, path);
}

/** File operations that write only the target Rust authorized. */
function authorizedFileOperations(name, target) {
  const write = async (path, content) => {
    if (resolve(path) !== target) throw new Error("Native file target changed after authorization");
    await writeFile(path, content, "utf-8");
  };
  return name === "write"
    ? { writeFile: write, mkdir: (directory) => mkdir(directory, { recursive: true }).then(() => {}) }
    : { readFile: (path) => readFile(path), writeFile: write,
      access: (path) => access(path, constants.R_OK | constants.W_OK) };
}

const FILE_WRITERS = { write: createWriteToolDefinition, edit: createEditToolDefinition };

/** Native Pi tools operate on the selected workspace while Pi itself keeps a private runtime CWD.
 * `authorize(name, target, signal, id)` asks Rust before every execution; `target` is the
 * resolved file for read, write and edit; Rust owns exported-text trust and instruction guards. */
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
        const target = writer || definition.name === "read"
          ? nativeTargetPath(workspaceRoot, input?.path) : undefined;
        const permission = authorize ? await authorize(definition.name, target, signal, id) : undefined;
        const boundary = permission?.untrusted_text;
        const deliver = (result) => boundary ? {
          ...result,
          content: result.content.map((part) => part.type === "text"
            ? { ...part, text: `${boundary.prefix}${part.text}${boundary.suffix}` } : part),
        } : result;
        const run = writer && target !== undefined
          ? writer(workspaceRoot, { operations: authorizedFileOperations(definition.name, target) })
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
