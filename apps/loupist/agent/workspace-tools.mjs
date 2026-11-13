import {
  createEditToolDefinition,
  createFindToolDefinition,
  createGrepToolDefinition,
  createLsToolDefinition,
  createReadToolDefinition,
  createWriteToolDefinition,
  detectSupportedImageMimeTypeFromFile,
} from "@earendil-works/pi-coding-agent";
import { constants, lstatSync, readlinkSync, realpathSync } from "node:fs";
import { spawn } from "node:child_process";
import { access, mkdir, open } from "node:fs/promises";
import { homedir } from "node:os";
import { basename, dirname, isAbsolute, join, matchesGlob, relative, resolve, sep } from "node:path";
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
function authorizedFileOperations(name, expected, identity) {
  const write = async (path, content) => {
    if (!sameTarget(canonicalTargetPath(path), expected)) {
      throw new Error("Native file target changed after authorization");
    }
    const flags = identity === null ? constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL
      : constants.O_WRONLY | (identity === undefined ? constants.O_CREAT : 0);
    const handle = await open(expected, flags | (constants.O_NOFOLLOW ?? 0) | (constants.O_NONBLOCK ?? 0));
    try {
      await verifyHandle(handle, identity);
      await handle.truncate(0);
      await handle.writeFile(content, "utf-8");
    } finally { await handle.close(); }
  };
  return name === "write"
    ? { writeFile: write, mkdir: async (directory) => {
      if (!sameTarget(canonicalTargetPath(directory), dirname(expected))) {
        throw new Error("Native file target changed after authorization");
      }
      await mkdir(dirname(expected), { recursive: true });
    } }
    : { readFile: () => pinnedReadOperations(expected, identity).readFile(), writeFile: write,
      access: (path) => access(path, constants.R_OK | constants.W_OK) };
}

async function verifyHandle(handle, identity) {
  const stat = await handle.stat({ bigint: true });
  if (!stat.isFile() || stat.nlink > 1n || (identity != null &&
    (stat.dev !== BigInt(identity.dev) || stat.ino !== BigInt(identity.ino)))) {
    throw new Error("Native file target changed after authorization");
  }
}

/** Read operations pinned to the file Rust authorized: the canonical path is
 * opened, the open descriptor's `(dev, ino)` must match what Rust saw, and the
 * bytes come through that descriptor, so a link swapped in after
 * authorization cannot redirect the read. `identity` is null when no file was
 * at the path when Rust decided, so nothing that appears later is read. */
function pinnedReadOperations(expected, identity) {
  const missing = () => Object.assign(new Error(`ENOENT: no such file or directory, open '${expected}'`), { code: "ENOENT" });
  return {
    access: async () => { if (identity === null) throw missing(); },
    detectImageMimeType: async () => {
      if (identity === null) throw missing();
      const handle = await open(expected, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0) | (constants.O_NONBLOCK ?? 0));
      try {
        await verifyHandle(handle, identity);
        // Pi's detector opens its argument. On the supported Unix hosts this
        // reopens the verified descriptor, so sniffing cannot follow a swap.
        return await detectSupportedImageMimeTypeFromFile(process.platform === "win32" ? expected : `/dev/fd/${handle.fd}`);
      } finally { await handle.close(); }
    },
    readFile: async () => {
      if (identity === null) throw missing();
      const handle = await open(expected, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0) | (constants.O_NONBLOCK ?? 0));
      try {
        await verifyHandle(handle, identity);
        return await handle.readFile();
      } finally {
        await handle.close();
      }
    },
  };
}

const FILE_WRITERS = { write: createWriteToolDefinition, edit: createEditToolDefinition };
const SEARCHERS = new Set(["ls", "find", "grep"]);

const MAX_SEARCH_BYTES = 50 * 1024;

/** Rust admits every entry, including ignore files, before any content search.
 * rg receives an identity-checked descriptor on stdin, never a host path to
 * walk. A stale or missing manifest fails closed. */
async function searchWorkspace(name, input, permission, signal) {
  const scope = permission?.search;
  if (!scope || !Array.isArray(scope.entries) || typeof permission.resolved !== "string") {
    throw new Error("Native search scope is unavailable");
  }
  if (signal?.aborted) throw new Error("Operation aborted");
  if (name === "ls" && !scope.directory) throw new Error("Not a directory");
  const root = permission.resolved;
  const limit = Math.min(10000, Math.max(1, input.limit ?? (name === "grep" ? 100 : name === "ls" ? 500 : 1000)));
  const lines = [];
  let bytes = 0;
  let count = 0;
  let truncated = scope.truncated === true;
  const glob = name === "find" ? input.pattern : input.glob;
  const matches = (path) => !glob || matchesGlob(glob.includes("/") ? relative(root, path) : basename(path), glob);
  for (const entry of scope.entries) {
    if (signal?.aborted) throw new Error("Operation aborted");
    if (count >= limit || bytes >= MAX_SEARCH_BYTES) { truncated = true; break; }
    if (typeof entry.path !== "string" || !matches(entry.path)) continue;
    const label = scope.directory ? relative(root, entry.path).replaceAll("\\", "/") : basename(entry.path);
    if (name !== "grep") {
      const line = label + (name === "ls" && entry.directory ? "/" : "");
      lines.push(line); bytes += Buffer.byteLength(line) + 1; count++;
      continue;
    }
    if (entry.directory) continue;
    const handle = await open(entry.path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0) | (constants.O_NONBLOCK ?? 0));
    try {
      await verifyHandle(handle, entry.identity);
      const args = ["--no-config", "--json", "--color=never", "--max-count", String(limit - count)];
      if (input.ignoreCase) args.push("--ignore-case");
      if (input.literal) args.push("--fixed-strings");
      if (input.context > 0) args.push("--context", String(Math.min(100, input.context)));
      args.push("--", input.pattern, "-");
      const remaining = MAX_SEARCH_BYTES - bytes;
      const result = await grepDescriptor(handle.fd, args, signal, remaining);
      for (const event of result.events) {
        if (event.type !== "match" && event.type !== "context") continue;
        const text = event.data?.lines?.text;
        if (typeof text !== "string") continue;
        const line = `${label}:${event.data.line_number}: ${text.replace(/\r?\n$/, "").slice(0, 2000)}`;
        lines.push(line); bytes += Buffer.byteLength(line) + 1;
        if (event.type === "match") count++;
      }
      truncated ||= result.truncated;
    } finally { await handle.close(); }
  }
  let text = lines.join("\n") || (name === "grep" ? "No matches found" : name === "find" ? "No files found matching pattern" : "(empty directory)");
  if (Buffer.byteLength(text) > MAX_SEARCH_BYTES) { text = Buffer.from(text).subarray(0, MAX_SEARCH_BYTES).toString("utf8"); truncated = true; }
  if (truncated || count >= limit) text += `\n\n[Search limit reached; narrow the path or pattern.]`;
  return { content: [{ type: "text", text }], details: {
    ...(count >= limit ? { resultLimitReached: limit } : {}),
    ...(truncated ? { truncated: true } : {}),
  } };
}

function grepDescriptor(fd, args, signal, maxBytes) {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(new Error("Operation aborted"));
    const child = spawn(process.platform === "win32" ? "rg.exe" : "rg", args, {
      stdio: [fd, "pipe", "pipe"], env: allowedWorkspaceEnvironment(),
    });
    let output = "", errors = "", truncated = false, timedOut = false;
    const kill = () => child.kill("SIGKILL");
    const timer = setTimeout(() => { timedOut = true; kill(); }, 10000);
    signal?.addEventListener("abort", kill, { once: true });
    child.stdout.on("data", (chunk) => {
      // JSON framing can exceed rendered text size; cap both independently.
      if (Buffer.byteLength(output) + chunk.length > Math.max(maxBytes * 4, 16384)) { truncated = true; kill(); }
      else output += chunk.toString("utf8");
    });
    child.stderr.on("data", (chunk) => { errors = (errors + chunk).slice(0, 4096); });
    const cleanup = () => { clearTimeout(timer); signal?.removeEventListener("abort", kill); };
    child.once("error", (error) => { cleanup(); reject(error); });
    child.once("close", (code) => {
      cleanup();
      if (signal?.aborted) return reject(new Error("Operation aborted"));
      if (timedOut) return reject(new Error("Native search timed out"));
      if (!truncated && code !== 0 && code !== 1) return reject(new Error(errors || "Native search failed"));
      const events = output.split("\n").filter(Boolean).flatMap((line) => {
        try { return [JSON.parse(line)]; } catch { return []; }
      });
      resolve({ events, truncated });
    });
  });
}

/** What `native.authorize` is told about one call: the resolved file for read,
 * write and edit; the resolved search root for ls, find and grep (the
 * workspace by default, as Pi resolves it). */
export function nativeRequest(workspaceRoot, name, input) {
  if (FILE_WRITERS[name] || name === "read") {
    const path = nativeTargetPath(workspaceRoot, input?.path);
    return path === undefined ? {} : { path };
  }
  if (SEARCHERS.has(name)) return { path: nativeTargetPath(workspaceRoot, input?.path || ".") };
  return {};
}

/** Native Pi file tools operate on the selected workspace while Pi itself keeps a private runtime CWD.
 * `authorize(name, request, signal, id)` asks Rust before every execution with the
 * `nativeRequest` above plus the complete `input` for argument-text validation;
 * Rust owns the permission policy, exported-text trust, the
 * canonical target (`resolved`) a read, write or edit is pinned to and the
 * `identity` a read must find. Analysis runs through the broker tool `run_python`. */
export function registerWorkspaceTools(pi, workspaceRoot, authorize) {
  const definitions = [
    createReadToolDefinition(workspaceRoot),
    createWriteToolDefinition(workspaceRoot),
    createEditToolDefinition(workspaceRoot),
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
        const permission = authorize ? await authorize(definition.name, { ...request, input }, signal, id) : undefined;
        const boundary = permission?.untrusted_text;
        const deliver = (result) => boundary ? {
          ...result,
          content: result.content.map((part) => part.type === "text"
            ? { ...part, text: `${boundary.prefix}${part.text}${boundary.suffix}` } : part),
        } : result;
        const expected = typeof permission?.resolved === "string"
          ? permission.resolved : request.path === undefined ? undefined : canonicalTargetPath(request.path);
        if (SEARCHERS.has(definition.name)) {
          return deliver(await searchWorkspace(definition.name, input, permission, signal));
        }
        // A search runs on the canonical root Rust decided about, and a
        // read opens the file Rust decided about, by identity when the
        // host reports one.
        if (writer && expected !== undefined && !sameTarget(canonicalTargetPath(request.path), expected)) {
          throw new Error("Native file target changed after authorization");
        }
        const run = writer && expected !== undefined
          ? writer(workspaceRoot, { operations: authorizedFileOperations(definition.name, expected, permission?.identity) })
          : definition.name === "read" && expected !== undefined && permission?.identity !== undefined
            ? createReadToolDefinition(workspaceRoot, { operations: pinnedReadOperations(expected, permission.identity) })
            : definition;
        const target = writer && expected !== undefined ? { ...input, path: expected } : input;
        const result = await run.execute(id, target, signal,
          onUpdate ? (update) => onUpdate(deliver(update)) : undefined, {
          ...ctx,
          cwd: workspaceRoot,
        });
        return deliver(result);
      },
    });
  }
}
