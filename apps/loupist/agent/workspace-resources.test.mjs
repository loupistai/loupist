import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs, { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { createAgentSession, DefaultResourceLoader, loadSkills, ModelRuntime, SessionManager, SettingsManager } from "@earendil-works/pi-coding-agent";
import { fauxAssistantMessage, fauxProvider, InMemoryCredentialStore } from "@earendil-works/pi-ai";

import { applyWorkspacePrompt, loadWorkspaceResources, registerWorkspaceResources, validExplicitSkillNames } from "./workspace-resources.mjs";

function fixture(t) {
  const temporary = realpathSync(mkdtempSync(join(tmpdir(), "loupist-workspace-resources-")));
  const root = join(temporary, "selected workspace");
  mkdirSync(root);
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const write = (path, content) => {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
    return path;
  };
  const skill = (base, name, description = `Research with ${name}`) => write(
    join(base, name, "SKILL.md"),
    `---\nname: ${name}\ndescription: ${description}\n---\n\n# Research workflow\n\nCompare the supplied observations.\n`,
  );
  return { temporary, root, write, skill };
}

function snapshot(root, instructions = [], skills = [], global_root = root) {
  const resource = (path) => ({
    scope: path.startsWith(join(root, ".loupist") + "/") ? "project"
      : path.startsWith(join(root, ".agents") + "/") ? "agents"
        : path.startsWith(join(root, ".pi") + "/") ? "pi"
          : global_root !== root && path.startsWith(global_root + "/") ? "global" : "workspace",
    path,
    sha256: createHash("sha256").update(readFileSync(path)).digest("hex"),
  });
  const result = { root, global_root, instructions: instructions.map(resource), skills: skills.map(resource) };
  const digest = createHash("sha256").update("loupist-workspace/1\0").update(root).update("\0").update(global_root);
  for (const entry of [...result.instructions, ...result.skills]) {
    digest.update("\0").update(entry.scope).update("\0").update(entry.path).update("\0").update(entry.sha256);
  }
  return { ...result, digest: digest.digest("hex") };
}

function promptEvent(cwd, skills = []) {
  return {
    systemPromptOptions: {
      cwd,
      customPrompt: "Research the supplied data.",
      selectedTools: ["read", "bash"],
      toolSnippets: {},
      toolGuidelines: {},
      promptGuidelines: [],
      appendSystemPrompt: "",
      contextFiles: [{ path: join(cwd, "AGENTS.md"), content: "private runtime context" }],
      skills: [...skills],
      sections: { other_host_section: "stable host data" },
    },
  };
}

test("loads only snapshotted root instructions and native Skill metadata without admitting project configuration", (t) => {
  const { temporary, root, write, skill } = fixture(t);
  const instructionText = "# Research workspace\nUse the supplied reporting periods.\n";
  const instructions = write(join(root, "AGENTS.md"), instructionText);
  const approvedSkill = skill(join(root, ".agents/skills"), "period-comparison", "Compare fiscal periods");
  const ancestorInstructions = write(join(temporary, "AGENTS.md"), "ancestor instructions must not enter this workspace\n");
  write(join(root, "subfolder/AGENTS.md"), "nested instructions must not enter root context\n");
  skill(join(root, ".pi/skills"), "unselected-workflow");
  const settings = write(join(root, ".pi/settings.json"), "deliberately invalid project settings");
  write(join(root, ".pi/SYSTEM.md"), "unselected system prompt\n");
  write(join(root, ".pi/APPEND_SYSTEM.md"), "unselected append prompt\n");
  const extensionMarker = join(temporary, "extension-ran");
  write(join(root, ".pi/extensions/probe.mjs"),
    `import { writeFileSync } from 'node:fs'; writeFileSync(${JSON.stringify(extensionMarker)}, 'executed');\n`);

  const approved = snapshot(root, [instructions], [approvedSkill]);
  const originalRead = fs.readFileSync;
  const reads = [];
  const readMock = t.mock.method(fs, "readFileSync", (path, ...args) => {
    reads.push(String(path));
    return originalRead(path, ...args);
  });
  syncBuiltinESMExports();
  let resources;
  try {
    resources = loadWorkspaceResources(approved);
  } finally {
    readMock.mock.restore();
    syncBuiltinESMExports();
  }
  assert.equal(reads.includes(ancestorInstructions), false);
  assert.equal(resources.root, root);
  assert.deepEqual(resources.contextFiles, [{ path: instructions, content: instructionText }]);
  assert.deepEqual(resources.skills.map(({ name, description, filePath }) => ({ name, description, filePath })), [
    { name: "period-comparison", description: "Compare fiscal periods", filePath: approvedSkill },
  ]);
  assert.deepEqual(resources.diagnostics, []);
  assert.equal(readFileSync(settings, "utf8"), "deliberately invalid project settings");
  assert.equal(existsSync(extensionMarker), false);
});

test("snapshot verification stays sealed when callers change the original manifest", (t) => {
  const { root, write } = fixture(t);
  const instructions = write(join(root, "AGENTS.md"), "approved instructions\n");
  const approved = snapshot(root, [instructions]);
  const resources = loadWorkspaceResources(approved);
  write(instructions, "changed instructions\n");
  approved.instructions[0].sha256 = snapshot(root, [instructions]).instructions[0].sha256;
  assert.throws(() => resources.verify(), /changed/);
});

test("native Skill parsing cannot silently replace the approved bytes", (t) => {
  const { root, skill } = fixture(t);
  const skillFile = skill(join(root, ".agents/skills"), "checked-workflow");
  const approved = snapshot(root, [], [skillFile]);
  const originalRead = fs.readFileSync;
  let skillReads = 0;
  const readMock = t.mock.method(fs, "readFileSync", (path, ...args) => {
    const content = originalRead(path, ...args);
    if (String(path) === skillFile && ++skillReads === 1) {
      writeFileSync(skillFile, "changed during native discovery\n");
    }
    return content;
  });
  syncBuiltinESMExports();
  try {
    assert.throws(() => loadWorkspaceResources(approved), /changed/);
  } finally {
    readMock.mock.restore();
    syncBuiltinESMExports();
  }
});

test("a resource that grows after stat is read through a fixed size buffer", (t) => {
  const { root, write } = fixture(t);
  const instructions = write(join(root, "AGENTS.md"), "approved instructions\n");
  const approved = snapshot(root, [instructions]);
  const originalRead = fs.readSync;
  let readCapacity = 0;
  const readMock = t.mock.method(fs, "readSync", (descriptor, buffer, ...args) => {
    if (readCapacity === 0) writeFileSync(instructions, "x".repeat(1024 * 1024));
    readCapacity = Math.max(readCapacity, buffer.length);
    return originalRead(descriptor, buffer, ...args);
  });
  syncBuiltinESMExports();
  try {
    assert.throws(() => loadWorkspaceResources(approved), /changed/);
    assert.equal(readCapacity, 64 * 1024 + 1);
  } finally {
    readMock.mock.restore();
    syncBuiltinESMExports();
  }
});

test("native registry exposes manual-only Skills and keeps app Skills on name collisions", async (t) => {
  const { temporary, root, write, skill } = fixture(t);
  const manualSkill = write(join(root, ".agents/skills/manual-study/SKILL.md"),
    "---\nname: manual-study\ndescription: Start only when requested\ndisable-model-invocation: true\n---\n\nRun this explicitly requested study.\n");
  const projectResearch = skill(join(root, ".agents/skills"), "research", "Project research method");
  const appResearch = skill(join(temporary, "app-skills"), "research", "App research method");
  const resources = loadWorkspaceResources(snapshot(root, [], [manualSkill, projectResearch]));
  const runtime = join(temporary, "private-runtime");
  mkdirSync(runtime);
  const agentDir = join(temporary, "private-agent");
  mkdirSync(agentDir);
  const settingsManager = SettingsManager.inMemory({ compaction: { enabled: false } });
  let commands;
  const loader = new DefaultResourceLoader({
    cwd: runtime, agentDir, settingsManager,
    noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
    additionalSkillPaths: [appResearch],
    extensionFactories: [(pi) => {
      commands = () => pi.getCommands();
      const ensureUnchanged = registerWorkspaceResources(pi, resources);
      pi.on("before_agent_start", (event) => {
        ensureUnchanged();
        applyWorkspacePrompt(event, resources, "Test runtime context");
      });
    }],
  });
  await loader.reload();
  const modelRuntime = await ModelRuntime.create({
    credentials: new InMemoryCredentialStore(), modelsPath: null, refreshOnCreate: false,
  });
  const faux = fauxProvider();
  modelRuntime.registerNativeProvider(faux.provider);
  const { session } = await createAgentSession({
    cwd: runtime, agentDir, modelRuntime, model: faux.getModel(),
    settingsManager, resourceLoader: loader, sessionManager: SessionManager.inMemory(runtime), noTools: "all",
  });
  t.after(() => session.dispose());
  await session.bindExtensions({ mode: "rpc" });
  assert.deepEqual(loader.getSkills().skills.map(({ name, filePath }) => ({ name, filePath })), [
    { name: "research", filePath: appResearch },
    { name: "manual-study", filePath: manualSkill },
  ]);
  assert(commands().some((command) => command.name === "skill:manual-study" && command.source === "skill"));
  assert.equal(session.systemPrompt.includes("manual-study"), false);
  let sentContext;
  faux.setResponses([(context) => {
    sentContext = context;
    return fauxAssistantMessage("Study complete.");
  }]);
  await session.prompt("/skill:manual-study compare these rows");
  const user = sentContext.messages.find((message) => message.role === "user");
  const userText = user.content.map((block) => block.type === "text" ? block.text : "").join("\n");
  assert(userText.includes('<skill name="manual-study"'));
  assert(userText.includes("Run this explicitly requested study."));
  assert(userText.includes("compare these rows"));
  assert.equal(session.systemPrompt.includes("manual-study"), false);
});

test("host-selected Skills reach the provider once through the enabled native registry", async (t) => {
  const { temporary, root, write } = fixture(t);
  const method = (path, name, body, manual = false) => write(path,
    `---\nname: ${name}\ndescription: Method for ${name}\n${manual ? "disable-model-invocation: true\n" : ""}---\n\n${body}\n`);
  const manual = method(join(root, ".agents/skills/manual-study/SKILL.md"), "manual-study", "EXPLICIT_MANUAL_BODY", true);
  const project = method(join(root, ".agents/skills/project-study/SKILL.md"), "project-study", "SELECTED_PROJECT_BODY");
  const shadow = method(join(root, ".agents/skills/research/SKILL.md"), "research", "SHADOW_PROJECT_BODY");
  const unselected = method(join(root, ".agents/skills/unselected-manual/SKILL.md"), "unselected-manual", "UNSELECTED_MANUAL_BODY", true);
  const disabled = method(join(root, ".pi/skills/disabled-study/SKILL.md"), "disabled-study", "DISABLED_BODY");
  const app = method(join(temporary, "app-skills/research/SKILL.md"), "research", "APP_RESEARCH_BODY");
  const runtime = join(temporary, "private-runtime");
  const saved = method(join(runtime, "skills/saved-report/SKILL.md"), "saved-report", "SAVED_REPORT_BODY", true);
  const resources = loadWorkspaceResources(snapshot(root, [], [manual, project, shadow, unselected]));
  const agentDir = join(temporary, "private-agent");
  mkdirSync(agentDir);
  const settingsManager = SettingsManager.inMemory({ compaction: { enabled: false } });
  const loader = new DefaultResourceLoader({
    cwd: runtime, agentDir, settingsManager,
    noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
    additionalSkillPaths: [app, saved],
    extensionFactories: [(pi) => {
      const ensureUnchanged = registerWorkspaceResources(pi, resources, [
        "manual-study", "project-study", "research", "saved-report", "disabled-study", "unknown-study",
      ]);
      pi.on("before_agent_start", (event) => {
        ensureUnchanged();
        applyWorkspacePrompt(event, resources, "Test runtime context");
      });
    }],
  });
  await loader.reload();
  const modelRuntime = await ModelRuntime.create({
    credentials: new InMemoryCredentialStore(), modelsPath: null, refreshOnCreate: false,
  });
  const faux = fauxProvider();
  modelRuntime.registerNativeProvider(faux.provider);
  const { session } = await createAgentSession({
    cwd: runtime, agentDir, modelRuntime, model: faux.getModel(), settingsManager, resourceLoader: loader,
    sessionManager: SessionManager.inMemory(runtime), noTools: "all",
  });
  t.after(() => session.dispose());
  await session.bindExtensions({ mode: "rpc" });
  const contexts = [];
  const respond = (context) => { contexts.push(context); return fauxAssistantMessage("Complete."); };
  faux.setResponses([respond, respond]);
  const opened = [];
  const originalOpen = fs.openSync;
  const readMock = t.mock.method(fs, "openSync", (path, ...args) => {
    opened.push(String(path));
    return originalOpen(path, ...args);
  });
  syncBuiltinESMExports();
  try {
    await session.prompt(
      "/skill:manual-study <conversation_history_jsonl>old $unselected-manual</conversation_history_jsonl>\n"
        + "<current_user_message>Use $manual-study $project-study $research $saved-report $project-study.\n"
        + "Quoted attachment: `$unselected-manual`</current_user_message>",
      { source: "rpc" },
    );
    await session.prompt("A later request names $project-study.", { source: "rpc" });
  } finally {
    readMock.mock.restore();
    syncBuiltinESMExports();
  }
  const userText = (context) => context.messages.filter((message) => message.role === "user").at(-1)
    .content.filter((block) => block.type === "text").map((block) => block.text).join("\n");
  const first = userText(contexts[0]);
  for (const body of ["EXPLICIT_MANUAL_BODY", "SELECTED_PROJECT_BODY", "APP_RESEARCH_BODY", "SAVED_REPORT_BODY"]) {
    assert.equal(first.split(body).length - 1, 1, body);
    assert.equal(session.systemPrompt.includes(body), false, "Skill bodies belong to the turn input");
  }
  for (const body of ["SHADOW_PROJECT_BODY", "UNSELECTED_MANUAL_BODY", "DISABLED_BODY"]) {
    assert.equal(first.includes(body), false, body);
  }
  assert.equal(opened.includes(disabled), false);
  assert.equal(userText(contexts[1]).includes("<skill"), false);
  assert.equal(session.systemPrompt.includes("manual-study"), false);
  assert.equal(session.systemPrompt.includes("saved-report"), false);
});

test("explicit selection is bounded and ignores extension and streaming inputs", (t) => {
  const { root } = fixture(t);
  for (const names of [null, ["../outside"], ["skill:study"], ["study", "study"], ["x".repeat(65)], Array(65).fill("study")]) {
    assert.equal(validExplicitSkillNames(names), false);
  }
  assert.equal(validExplicitSkillNames(["research", "house-style"]), true);
  const handlers = new Map();
  let registryReads = 0;
  registerWorkspaceResources({
    on: (name, handler) => handlers.set(name, handler),
    getCommands: () => { registryReads += 1; return []; },
  }, loadWorkspaceResources(snapshot(root)), ["research"]);
  const input = handlers.get("input");
  assert.deepEqual(input({ source: "extension", text: "$research" }), { action: "continue" });
  assert.deepEqual(input({ source: "rpc", streamingBehavior: "steer", text: "$research" }), { action: "continue" });
  assert.deepEqual(input({ source: "rpc", streamingBehavior: "followUp", text: "$research" }), { action: "continue" });
  assert.equal(registryReads, 0);
  assert.deepEqual(input({ source: "rpc", text: "Host envelope" }), { action: "continue" });
  assert.equal(registryReads, 1);
  assert.deepEqual(input({ source: "rpc", text: "$research" }), { action: "continue" });
  assert.equal(registryReads, 1);
});

test("changed resources terminate the sidecar before native expansion or provider work", (t) => {
  const { root, write } = fixture(t);
  const instructions = write(join(root, "AGENTS.md"), "approved instructions\n");
  const approved = snapshot(root, [instructions]);
  const moduleUrl = new URL("./workspace-resources.mjs", import.meta.url).href;
  for (const phase of ["resources_discover", "input", "before_agent_start"]) {
    write(instructions, "approved instructions\n");
    const probe = spawnSync(process.execPath, ["--input-type=module", "-e", `
      import { writeFileSync } from "node:fs";
      import { loadWorkspaceResources, registerWorkspaceResources } from ${JSON.stringify(moduleUrl)};
      const resources = loadWorkspaceResources(${JSON.stringify(approved)});
      const handlers = new Map();
      const ensure = registerWorkspaceResources({ on: (name, handler) => handlers.set(name, handler), getCommands: () => [] }, resources);
      writeFileSync(${JSON.stringify(instructions)}, "changed instructions\\n");
      ${phase === "before_agent_start" ? "ensure();" : `handlers.get(${JSON.stringify(phase)})();`}
      process.stdout.write("unexpected continuation");
    `], { encoding: "utf8", timeout: 10_000 });
    assert.equal(probe.status, 76, probe.stderr);
    assert.equal(probe.stdout, "");
    assert.match(probe.stderr, /workspace resources changed/);
  }
});

test("an empty instruction snapshot does not admit ancestor or unsnapshotted root context", (t) => {
  const { temporary, root, write } = fixture(t);
  write(join(temporary, "AGENTS.md"), "ancestor research instructions\n");
  write(join(root, "AGENTS.md"), "new instructions outside this snapshot\n");
  const resources = loadWorkspaceResources(snapshot(root));
  assert.deepEqual(resources.contextFiles, []);
  assert.deepEqual(resources.skills, []);
});

test("changed instruction or Skill bytes invalidate the old snapshot and fresh snapshots use the change", (t) => {
  const { root, write, skill } = fixture(t);
  const instructions = write(join(root, "AGENTS.md"), "first instruction\n");
  const skillFile = skill(join(root, ".agents/skills"), "valuation", "First valuation method");
  const original = snapshot(root, [instructions], [skillFile]);
  write(instructions, "other instruction\n");
  assert.throws(() => loadWorkspaceResources(original), /changed/);
  write(instructions, "first instruction\n");
  skill(join(root, ".agents/skills"), "valuation", "Other valuation method");
  assert.throws(() => loadWorkspaceResources(original), /changed/);
  const refreshed = loadWorkspaceResources(snapshot(root, [instructions], [skillFile]));
  assert.equal(refreshed.skills[0].description, "Other valuation method");
  assert.throws(() => loadWorkspaceResources({ ...snapshot(root), digest: "0".repeat(64) }), /digest/);
});

test("valid hashes cannot admit resources outside the selected folder", (t) => {
  const { temporary, root, write, skill } = fixture(t);
  const outsideInstruction = write(join(temporary, "AGENTS.md"), "outside instructions\n");
  const outsideSkill = skill(join(temporary, "external-skills"), "outside-workflow");
  assert.throws(() => loadWorkspaceResources(snapshot(root, [outsideInstruction])), /escaped|scope/);
  assert.throws(() => loadWorkspaceResources(snapshot(root, [], [outsideSkill])), /escaped|scope/);
});

test("resource files and parent directories cannot redirect through symlinks", { skip: process.platform === "win32" }, (t) => {
  const { temporary, root, write, skill } = fixture(t);
  const outsideInstruction = write(join(temporary, "external-instructions.md"), "outside instructions\n");
  const linkedInstruction = join(root, "AGENTS.md");
  symlinkSync(outsideInstruction, linkedInstruction);
  assert.throws(() => loadWorkspaceResources(snapshot(root, [linkedInstruction])), /changed/);

  const outsideSkill = skill(join(temporary, "external-skills"), "linked-workflow");
  const linkedDirectory = join(root, ".agents/skills/linked-workflow");
  mkdirSync(dirname(linkedDirectory), { recursive: true });
  symlinkSync(dirname(outsideSkill), linkedDirectory, "dir");
  assert.throws(() => loadWorkspaceResources(snapshot(root, [], [join(linkedDirectory, "SKILL.md")])), /changed/);

  const linkedRoot = join(temporary, "linked-workspace");
  symlinkSync(root, linkedRoot, "dir");
  assert.throws(() => loadWorkspaceResources(snapshot(linkedRoot)), /changed/);
});

test("every bundled Skill loads as a Pi Skill under its folder name", () => {
  // Pi parses frontmatter as YAML: an unquoted ": " in a description drops the Skill.
  const bundled = join(dirname(new URL(import.meta.url).pathname), "skills");
  const folders = fs.readdirSync(bundled, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && existsSync(join(bundled, entry.name, "SKILL.md")))
    .map((entry) => entry.name);
  assert.ok(folders.length > 0);
  const { skills, diagnostics } = loadSkills({
    cwd: bundled, agentDir: bundled, includeDefaults: false,
    skillPaths: folders.map((name) => join(bundled, name, "SKILL.md")),
  });
  assert.deepEqual(diagnostics.filter((item) => item.type !== "collision"), []);
  assert.deepEqual(skills.map((skill) => skill.name).sort(), [...folders].sort());
  for (const skill of skills) assert.ok(skill.description.trim().length > 0, skill.name);
});

test("structured workspace prompts preserve app Skills and remain stable across private runtime directories", (t) => {
  const { temporary, root, write, skill } = fixture(t);
  const instructions = write(join(root, "AGENTS.md"), "# Workspace rules\nUse local research files.\n");
  const projectResearch = skill(join(root, ".agents/skills"), "research", "Project research method");
  const projectComparison = skill(join(root, ".agents/skills"), "comparison", "Project comparison method");
  const appSkillFile = skill(join(temporary, "app-skills"), "research", "App research method");
  const { skills: appSkills } = loadSkills({
    cwd: temporary, agentDir: temporary, skillPaths: [appSkillFile], includeDefaults: false,
  });
  const resources = loadWorkspaceResources(snapshot(root, [instructions], [projectResearch, projectComparison]));
  const first = promptEvent(join(temporary, "private-run-one"), appSkills);
  const second = promptEvent(join(temporary, "private-run-two"), appSkills);
  const hostContext = "Loupist: sample\nReporting currency: USD";
  applyWorkspacePrompt(first, resources, hostContext);
  applyWorkspacePrompt(second, resources, hostContext);

  assert.deepEqual(first.systemPromptOptions, second.systemPromptOptions);
  assert.equal(first.systemPromptOptions.cwd, root);
  assert.deepEqual(first.systemPromptOptions.contextFiles, resources.contextFiles);
  assert.deepEqual(first.systemPromptOptions.skills.map(({ name, filePath }) => ({ name, filePath })), [
    { name: "research", filePath: appSkillFile },
    { name: "comparison", filePath: projectComparison },
  ]);
  assert.equal(first.systemPromptOptions.sections.loupist_runtime, hostContext);
  assert.equal(first.systemPromptOptions.sections.other_host_section, "stable host data");

  // Reapplying on a warm turn is idempotent.
  applyWorkspacePrompt(first, resources, hostContext);
  assert.deepEqual(first.systemPromptOptions, second.systemPromptOptions);
});

test("global and project scopes preserve instruction order and Skill collision precedence", (t) => {
  const { temporary, root, write, skill } = fixture(t);
  const global = join(temporary, "global-loupist");
  const instructions = [
    write(join(global, "AGENTS.md"), "Global guidance"),
    write(join(root, "AGENTS.md"), "Workspace guidance"),
    write(join(root, ".loupist/AGENTS.md"), "Project guidance"),
  ];
  const roots = [join(root, ".loupist/skills"), join(root, ".agents/skills"), join(root, ".pi/skills"), join(global, "skills")];
  const skills = roots.flatMap((base, index) => [
    skill(base, "same-method", `Scope ${index}`),
    skill(base, `scope-${index}`),
  ]);
  const manifest = snapshot(root, instructions, skills, global);
  const resources = loadWorkspaceResources(manifest);
  assert.deepEqual(resources.contextFiles.map((file) => file.content), ["Global guidance", "Workspace guidance", "Project guidance"]);
  assert.equal(resources.skills.find((item) => item.name === "same-method").filePath, skills[0]);
  assert.equal(resources.skills.length, 5);
  write(instructions[0], "New global guidance");
  assert.throws(() => resources.verify(), /changed/);
  assert.throws(() => loadWorkspaceResources(manifest), /changed/);
  assert.equal(loadWorkspaceResources(snapshot(root, instructions, skills, global)).contextFiles[0].content, "New global guidance");
});

test("scope paths and total file budget are verified independently from content hashes", (t) => {
  const { temporary, root, write, skill } = fixture(t);
  const global = join(temporary, "global-loupist");
  const globalInstruction = write(join(global, "AGENTS.md"), "Global guidance");
  const projectInstruction = write(join(root, ".loupist/AGENTS.md"), "Project guidance");
  const approved = snapshot(root, [globalInstruction, projectInstruction], [], global);
  approved.instructions[0].scope = "project";
  assert.throws(() => loadWorkspaceResources(approved), /scope|escaped/);
  const skills = Array.from({ length: 64 }, (_, i) => skill(join(root, ".loupist/skills"), `test-${i}`));
  assert.throws(() => loadWorkspaceResources(snapshot(root, [globalInstruction], skills, global)), /bounded/);
});
