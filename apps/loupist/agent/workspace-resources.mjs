import { createHash } from "node:crypto";
import { closeSync, constants, fstatSync, lstatSync, openSync, readSync, realpathSync } from "node:fs";
import { basename, dirname, isAbsolute, relative, resolve } from "node:path";
import { loadSkills, parseFrontmatter } from "@earendil-works/pi-coding-agent";

const MAX_RESOURCE_BYTES = 64 * 1024;
const MAX_RESOURCES_BYTES = 256 * 1024;
const MAX_RESOURCE_FILES = 64;

function boundedResource(resource, roots, kind) {
  if (!resource || typeof resource.path !== "string" || !isAbsolute(resource.path)
      || typeof resource.sha256 !== "string" || !/^[a-f0-9]{64}$/u.test(resource.sha256)) {
    throw new Error("Workspace resource snapshot is invalid");
  }
  const root = roots[resource.scope];
  if (!root) throw new Error("Workspace resource scope is invalid");
  const path = resolve(resource.path);
  const child = relative(root, path);
  if (!child || child === ".." || child.startsWith("../") || child.startsWith("..\\") || isAbsolute(child)) {
    throw new Error("Workspace resource escaped its selected folder");
  }
  const parts = child.split(/[\\/]/u);
  const allowed = kind === "instructions"
    ? parts.length === 1 && (resource.scope === "workspace"
      ? ["AGENTS.override.md", "AGENTS.md", "AGENTS.MD", "CLAUDE.md", "CLAUDE.MD"].includes(child)
      : ["global", "project"].includes(resource.scope) && child === "AGENTS.md")
    : resource.scope !== "workspace" && parts[0] === "skills"
      && (parts.length === 2 ? parts[1].endsWith(".md") : parts.at(-1) === "SKILL.md");
  if (!allowed) throw new Error("Workspace resource is outside its allowed scope");
  const { sha256, ...loaded } = boundedFile(path);
  if (sha256 !== resource.sha256) {
    throw new Error("Workspace resource changed before the turn started");
  }
  return loaded;
}

function boundedFile(path) {
  if (typeof path !== "string" || !isAbsolute(path) || resolve(path) !== path) {
    throw new Error("Skill resource path is invalid");
  }
  const metadata = lstatSync(path);
  if (metadata.isSymbolicLink() || !metadata.isFile() || metadata.size > MAX_RESOURCE_BYTES
      || realpathSync(path) !== path) {
    throw new Error("Workspace resource changed before the turn started");
  }
  let descriptor;
  let bytes;
  try {
    const noFollow = process.platform === "win32" ? 0 : (constants.O_NOFOLLOW ?? 0);
    descriptor = openSync(path, constants.O_RDONLY | noFollow);
    const opened = fstatSync(descriptor);
    if (!opened.isFile() || opened.size !== metadata.size) {
      throw new Error("Workspace resource changed before the turn started");
    }
    const buffer = Buffer.alloc(MAX_RESOURCE_BYTES + 1);
    let length = 0;
    while (length < buffer.length) {
      const read = readSync(descriptor, buffer, length, buffer.length - length, null);
      if (read === 0) break;
      length += read;
    }
    bytes = buffer.subarray(0, length);
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
  }
  if (bytes.length !== metadata.size) {
    throw new Error("Workspace resource changed before the turn started");
  }
  return {
    path, content: new TextDecoder("utf-8", { fatal: true }).decode(bytes), bytes: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

/** Rust snapshots the allowed text files before selecting a Pi session cache.
 * Native Pi parses those exact files; user settings and JS extensions are not
 * part of this path. Changed bytes fail this launch and are resnapshotted on the
 * next turn, so an old session never silently gains changed instructions.
 */
export function loadWorkspaceResources(snapshot) {
  if (!snapshot || typeof snapshot.root !== "string" || !isAbsolute(snapshot.root)
      || typeof snapshot.global_root !== "string" || !isAbsolute(snapshot.global_root)
      || !Array.isArray(snapshot.instructions) || snapshot.instructions.length > 3
      || !Array.isArray(snapshot.skills) || snapshot.skills.length > MAX_RESOURCE_FILES
      || typeof snapshot.digest !== "string" || !/^[a-f0-9]{64}$/u.test(snapshot.digest)) {
    throw new Error("Loupist workspace is unavailable");
  }
  const root = realpathSync(snapshot.root);
  if (root !== snapshot.root || !lstatSync(root).isDirectory()) {
    throw new Error("Loupist workspace changed before the turn started");
  }
  const globalRoot = realpathSync(snapshot.global_root);
  if (globalRoot !== snapshot.global_root || !lstatSync(globalRoot).isDirectory()) {
    throw new Error("Loupist global resources changed before the turn started");
  }
  const roots = Object.freeze({ global: globalRoot, workspace: root,
    project: resolve(root, ".loupist"), agents: resolve(root, ".agents"), pi: resolve(root, ".pi") });
  const instructionOrder = ["global", "workspace", "project"];
  const skillOrder = ["project", "agents", "pi", "global"];
  for (const [entries, order] of [[snapshot.instructions, instructionOrder], [snapshot.skills, skillOrder]]) {
    let previous = -1;
    const seen = new Set();
    for (const entry of entries) {
      const index = order.indexOf(entry.scope);
      if (index < previous || index < 0 || (order === instructionOrder && seen.has(index))) {
        throw new Error("Workspace resource scope order is invalid");
      }
      previous = index;
      seen.add(index);
    }
  }
  const instructions = snapshot.instructions.map((entry) => boundedResource(entry, roots, "instructions"));
  const skillFiles = snapshot.skills.map((entry) => boundedResource(entry, roots, "skills"));
  const resources = [...instructions, ...skillFiles];
  if (resources.length > MAX_RESOURCE_FILES
      || resources.reduce((sum, resource) => sum + resource.bytes, 0) > MAX_RESOURCES_BYTES
      || new Set(resources.map((resource) => resource.path)).size !== resources.length) {
    throw new Error("Workspace resources exceed their bounded snapshot");
  }
  const digest = createHash("sha256").update("loupist-workspace/1\0").update(root).update("\0").update(globalRoot);
  for (const entry of [...snapshot.instructions, ...snapshot.skills]) {
    digest.update("\0").update(entry.scope).update("\0").update(entry.path).update("\0").update(entry.sha256);
  }
  if (digest.digest("hex") !== snapshot.digest) {
    throw new Error("Workspace resource snapshot digest is invalid");
  }
  // Pi's context-file discovery walks ancestors before callers can filter its
  // result. Use the verified root bytes directly, without reading other files.
  const contextFiles = instructions.map(({ path, content }) => ({ path, content }));
  const sealedFiles = [
    ...snapshot.instructions.map((entry) => ({ ...entry, kind: "instructions" })),
    ...snapshot.skills.map((entry) => ({ ...entry, kind: "skills" })),
  ];
  const verify = () => {
    if (realpathSync(root) !== root || !lstatSync(root).isDirectory()
        || realpathSync(globalRoot) !== globalRoot || !lstatSync(globalRoot).isDirectory()) {
      throw new Error("Loupist workspace changed before the turn started");
    }
    for (const entry of sealedFiles) boundedResource(entry, roots, entry.kind);
  };
  const { skills, diagnostics } = loadSkills({
    cwd: root,
    agentDir: process.cwd(),
    skillPaths: skillFiles.map((entry) => entry.path),
    includeDefaults: false,
  });
  // Discovery is text-only. Recheck bytes after native parsing as well.
  verify();
  return { root, globalRoot, contextFiles, skills, diagnostics, verify };
}

/** Register approved files in Pi's native registry, which expands /skill:name
 * before before_agent_start, including manual-only Skills. Extension hook
 * exceptions are swallowed by Pi, so a changed snapshot must end the sidecar.
 */
export function validExplicitSkillNames(names) {
  return Array.isArray(names) && names.length <= MAX_RESOURCE_FILES
    && new Set(names).size === names.length
    && names.every((name) => typeof name === "string" && /^[a-z0-9-]{1,64}$/u.test(name));
}

function escapeXml(value) {
  return value.replace(/&/gu, "&amp;").replace(/</gu, "&lt;").replace(/>/gu, "&gt;")
    .replace(/"/gu, "&quot;").replace(/'/gu, "&apos;");
}

function expandExplicitSkills(text, names, commands) {
  const nativeSelector = text.match(/^\/skill:([^\s]+)(?:\s|$)/u);
  const registry = new Map(commands.filter((command) => command.source === "skill")
    .map((command) => [command.name, command]));
  const blocks = [];
  let totalBytes = 0;
  for (const name of names) {
    // Native Pi expands the leading selector after this hook. Do not inject
    // the same method a second time when the user also names it with $.
    if (nativeSelector?.[1] === name) continue;
    const command = registry.get(`skill:${name}`);
    if (!command) continue;
    const file = boundedFile(command.sourceInfo?.path);
    totalBytes += file.bytes;
    if (totalBytes > MAX_RESOURCES_BYTES) throw new Error("Selected Skills exceed their bounded input");
    const { frontmatter, body } = parseFrontmatter(file.content);
    const parsedName = typeof frontmatter.name === "string" ? frontmatter.name : basename(dirname(file.path));
    if (parsedName !== name) throw new Error("Selected Skill changed after discovery");
    blocks.push(`<skill name="${escapeXml(name)}" location="${escapeXml(file.path)}">\nReferences are relative to ${escapeXml(dirname(file.path))}.\n\n${body.trim()}\n</skill>`);
  }
  if (blocks.length === 0) return text;
  const prefix = blocks.join("\n\n");
  // Keep /skill at the beginning so Pi's native expansion still runs.
  return nativeSelector
    ? `${nativeSelector[0].trim()} ${prefix}\n\n${text.slice(nativeSelector[0].length)}`
    : `${prefix}\n\n${text}`;
}

export function registerWorkspaceResources(pi, resources, explicitSkillNames = []) {
  if (!validExplicitSkillNames(explicitSkillNames)) throw new Error("Explicit Skill selection is invalid");
  const selectedNames = [...explicitSkillNames];
  let initialRpcInput = true;
  const ensureUnchanged = () => {
    try {
      resources.verify();
    } catch {
      process.stderr.write("Loupist workspace resources changed; start a new turn.\n");
      process.exit(76);
    }
  };
  pi.on("resources_discover", () => {
    ensureUnchanged();
    const existingNames = new Set(pi.getCommands()
      .filter((command) => command.source === "skill")
      .map((command) => command.name));
    return {
      skillPaths: resources.skills
        .filter((skill) => !existingNames.has(`skill:${skill.name}`))
        .map((skill) => skill.filePath),
    };
  });
  pi.on("input", (event) => {
    // This runs after native discovery and before explicit Skill expansion.
    ensureUnchanged();
    if (!initialRpcInput || event?.source !== "rpc" || event.streamingBehavior) {
      return { action: "continue" };
    }
    initialRpcInput = false;
    try {
      const expanded = expandExplicitSkills(event.text, selectedNames, pi.getCommands());
      ensureUnchanged();
      if (expanded !== event.text) return { action: "transform", text: expanded };
    } catch {
      // Pi swallows hook errors. Do not continue without an explicitly selected
      // method after its registered file became invalid or exceeded the budget.
      process.stderr.write("Loupist selected Skill changed or is unavailable; start a new turn.\n");
      process.exit(76);
    }
    return { action: "continue" };
  });
  return ensureUnchanged;
}

/** Keep the provider prefix stable: Pi can diff these structured sections,
 * while live time, selected items, and working notes stay in the turn message.
 */
export function applyWorkspacePrompt(event, resources, runtimeContext) {
  const options = event.systemPromptOptions;
  if (!options || !Array.isArray(options.skills) || !Array.isArray(options.contextFiles)
      || !options.sections || typeof options.sections !== "object") {
    throw new Error("Pi structured prompt support is required");
  }
  options.cwd = resources.root;
  options.contextFiles = resources.contextFiles;
  const names = new Set(options.skills.map((skill) => skill.name));
  for (const skill of resources.skills) {
    // App-owned method cards retain their contract; project skills can add
    // workflows without silently replacing a bundled skill of the same name.
    if (!names.has(skill.name)) {
      options.skills.push(skill);
      names.add(skill.name);
    }
  }
  options.sections.loupist_runtime = runtimeContext;
  options.sections.loupist_skill_folders = `Skill folders (instructions and supporting resources):\nPersonal: ${resolve(resources.globalRoot, "skills")}\nThis Loupist: ${resolve(resources.root, ".loupist/skills")}\nUse this Loupist unless the user requests personal scope. New skills load on the next turn.`;
}
