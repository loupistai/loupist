import {
  createBashToolDefinition,
  createEditToolDefinition,
  createFindToolDefinition,
  createGrepToolDefinition,
  createLsToolDefinition,
  createReadToolDefinition,
  createWriteToolDefinition,
  getShellConfig,
} from "@earendil-works/pi-coding-agent";
import { spawn } from "node:child_process";
import { constants, lstatSync, readlinkSync, realpathSync } from "node:fs";
import { access, mkdir, readFile, writeFile } from "node:fs/promises";
import { constants as osConstants, homedir } from "node:os";
import { basename, dirname, isAbsolute, join, resolve, sep } from "node:path";
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

/** Symlink hops a target may take before it counts as a loop (as Rust's resolver). */
const MAX_LINK_HOPS = 40;

const isSymlink = (path) => lstatSync(path, { throwIfNoEntry: false })?.isSymbolicLink() === true;

/** The form Rust pins a write to: the longest existing ancestor's real path
 * plus the parts not yet on disk, so a link swapped in later changes it. A
 * dangling link is followed to where it points, since that is where a write
 * through it lands; a chain that loops or cannot be read throws. */
export function canonicalTargetPath(path) {
  let pending = resolve(path);
  for (let hop = 0; hop < MAX_LINK_HOPS; hop++) {
    const rest = [];
    let existing = pending;
    let real;
    for (;;) {
      try {
        real = realpathSync.native(existing);
        break;
      } catch {
        if (isSymlink(existing)) break;
        const parent = dirname(existing);
        if (parent === existing) return pending;
        rest.push(basename(existing));
        existing = parent;
      }
    }
    if (real !== undefined) {
      // `..` left in `rest` comes from a link's target and applies after the links before it.
      return rest.reverse().reduce((target, part) => part === ".." ? dirname(target) : join(target, part), real);
    }
    const target = readlinkSync(existing);
    const parent = realpathSync.native(dirname(existing));
    pending = rest.reverse().reduce((target, part) => `${target}${sep}${part}`, isAbsolute(target) ? target : `${parent}${sep}${target}`);
  }
  throw new Error("Native file target cannot be resolved");
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

/** How long after the shell exits to wait for descendants holding its pipes. */
const PIPE_GRACE_MS = 500;

const isStringMap = (value) => typeof value === "object" && value !== null && !Array.isArray(value)
  && Object.values(value).every((entry) => typeof entry === "string");

/** Bash operations that run every command inside the sandbox Rust returned
 * from `native.authorize`: its argv prefix, then the shell and its arguments,
 * then the command, each as its own argument, so nothing is re-quoted. A
 * malformed sandbox throws, so a command Rust meant to confine never runs
 * unconfined. As Pi's local operations, output streams and an abort or
 * timeout kills the whole process group. */
export function sandboxedBashOperations(sandbox) {
  const argv = sandbox?.argv;
  if (!Array.isArray(argv) || argv.length === 0 || !argv.every((part) => typeof part === "string" && part.length > 0)
    || (sandbox.env !== undefined && !isStringMap(sandbox.env))) {
    throw new Error("Native Bash sandbox is invalid, so the command does not run");
  }
  const [program, ...prefix] = argv;
  const shell = getShellConfig();
  return {
    exec: (command, cwd, { onData, signal, timeout, env }) => new Promise((resolve, reject) => {
      if (signal?.aborted) return reject(new Error("aborted"));
      const child = spawn(program, [...prefix, shell.shell, ...shell.args, command], {
        cwd, detached: true, env: { ...env, ...sandbox.env }, stdio: ["ignore", "pipe", "pipe"],
      });
      const kill = () => {
        if (!child.pid) return;
        try { process.kill(-child.pid, "SIGKILL"); } catch {
          try { process.kill(child.pid, "SIGKILL"); } catch { /* already gone */ }
        }
      };
      let timedOut = false;
      let settled = false;
      let graceTimer;
      const timer = timeout === undefined ? undefined : setTimeout(() => { timedOut = true; kill(); }, timeout * 1000);
      signal?.addEventListener("abort", kill, { once: true });
      child.stdout.on("data", onData);
      child.stderr.on("data", onData);
      const settle = (outcome) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        clearTimeout(graceTimer);
        signal?.removeEventListener("abort", kill);
        child.stdout.destroy();
        child.stderr.destroy();
        outcome();
      };
      const finish = (code, signalName) => settle(() => {
        if (signal?.aborted) return reject(new Error("aborted"));
        if (timedOut) return reject(new Error(`timeout:${timeout}`));
        resolve({ exitCode: code ?? (signalName ? 128 + (osConstants.signals[signalName] ?? 0) : 1) });
      });
      child.once("error", (error) => settle(() => reject(error)));
      child.once("close", finish);
      // A background descendant can hold the pipes open after the shell exits.
      child.once("exit", (code, signalName) => { graceTimer = setTimeout(() => finish(code, signalName), PIPE_GRACE_MS); });
    }),
  };
}

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
  const bashOptions = {
    exposeSessionEnvironment: false,
    spawnHook: ({ command, env }) => ({
      command,
      cwd: workspaceRoot,
      env: allowedWorkspaceEnvironment(env),
    }),
  };
  const definitions = [
    createReadToolDefinition(workspaceRoot),
    createWriteToolDefinition(workspaceRoot),
    createEditToolDefinition(workspaceRoot),
    createBashToolDefinition(workspaceRoot, bashOptions),
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
        // Rust names the sandbox a command must run in; without one the
        // command runs as before (a clean reply on a host with no sandbox).
        const run = writer && expected !== undefined
          ? writer(workspaceRoot, { operations: authorizedFileOperations(definition.name, expected) })
          : definition.name === "bash" && permission?.sandbox !== undefined
            ? createBashToolDefinition(workspaceRoot, { ...bashOptions, operations: sandboxedBashOperations(permission.sandbox) })
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
