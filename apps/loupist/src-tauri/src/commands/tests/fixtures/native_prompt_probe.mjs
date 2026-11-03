// Receives prompts and runtime profiles built by the Rust production path.
// Only a faux provider and temporary workspace participate in this probe.
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createAgentSession, DefaultResourceLoader, ModelRuntime, SessionManager, SettingsManager } from "@earendil-works/pi-coding-agent";
import { fauxAssistantMessage, fauxProvider, getSystemMessageText, InMemoryCredentialStore } from "@earendil-works/pi-ai";
import loupistTerminalExtension from "../../../../../agent/loupist-extension.mjs";

const cases = JSON.parse(readFileSync(0, "utf8"));
setTimeout(() => {
  process.stderr.write("Native prompt probe exceeded its deadline.\n");
  process.exit(1);
}, 15_000).unref();
const temporary = realpathSync(mkdtempSync(join(tmpdir(), "loupist-native-prompt-")));
const root = join(temporary, "workspace");
const skillDirectory = join(root, ".agents/skills/manual-study");
mkdirSync(skillDirectory, { recursive: true });
const skillPath = join(skillDirectory, "SKILL.md");
writeFileSync(skillPath, "---\nname: manual-study\ndescription: Explicit study only\ndisable-model-invocation: true\n---\n\nMANUAL_STUDY_BODY: compare the supplied observations.\n");
const skill = { scope: "agents", path: skillPath, sha256: createHash("sha256").update(readFileSync(skillPath)).digest("hex") };
const digest = createHash("sha256").update("loupist-workspace/1\0").update(root).update("\0").update(temporary)
  .update("\0").update(skill.scope).update("\0").update(skill.path).update("\0").update(skill.sha256).digest("hex");
const workspace = { root, global_root: temporary, instructions: [], skills: [skill], digest };
const systemPrompt = readFileSync(new URL("../../../../../agent/SYSTEM.md", import.meta.url), "utf8");
const results = [];

try {
  for (const [index, input] of cases.entries()) {
    const runtime = join(temporary, `runtime-${index}`);
    const agentDir = join(temporary, `agent-${index}`);
    mkdirSync(runtime);
    mkdirSync(agentDir);
    const hostContextPath = join(runtime, "host-context.json");
    writeFileSync(hostContextPath, JSON.stringify({ workspace, agent_runtime: input.runtime_profile }));
    process.env.LOUPIST_HOST_CONTEXT_FILE = hostContextPath;
    process.env.LOUPIST_MODEL_RUN_OPTIONS = "{}";
    const settingsManager = SettingsManager.inMemory({ compaction: { enabled: false } });
    const resourceLoader = new DefaultResourceLoader({
      cwd: runtime, agentDir, settingsManager, systemPrompt,
      noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
      extensionFactories: [loupistTerminalExtension],
    });
    await resourceLoader.reload();
    const modelRuntime = await ModelRuntime.create({
      credentials: new InMemoryCredentialStore(), modelsPath: null, refreshOnCreate: false,
    });
    const faux = fauxProvider();
    modelRuntime.registerNativeProvider(faux.provider);
    const { session } = await createAgentSession({
      cwd: runtime, agentDir, modelRuntime, model: faux.getModel(), settingsManager,
      resourceLoader, sessionManager: SessionManager.inMemory(runtime), noTools: "all",
    });
    try {
      const errors = [];
      await session.bindExtensions({ mode: "rpc", onError: (error) => errors.push(error.error) });
      let userText;
      let sentSystemPrompt;
      faux.setResponses([(context) => {
        sentSystemPrompt = context.messages.filter((message) => message.role === "system")
          .map(getSystemMessageText).join("\n");
        const user = context.messages.findLast((message) => message.role === "user");
        userText = user.content.filter((block) => block.type === "text").map((block) => block.text).join("\n");
        return fauxAssistantMessage("Probe completed.");
      }]);
      await session.prompt(input.prompt);
      if (errors.length) throw new Error(errors.join("; "));
      const runtimeSection = sentSystemPrompt?.match(/<loupist_runtime>\n([\s\S]*?)\n<\/loupist_runtime>/u);
      if (!runtimeSection || !userText) throw new Error("Provider did not receive the host prompt");
      const runtimeText = runtimeSection[1];
      const flag = (name) => {
        const match = runtimeText.match(new RegExp(`^${name}: (true|false)$`, "m"));
        return match ? match[1] === "true" : null;
      };
      const registeredTools = session.extensionRunner.getAllRegisteredTools().map((tool) => tool.definition.name);
      const load = session.extensionRunner.getToolDefinition("capability_load");
      let sourceComponentLoadable = true;
      try {
        await load.execute("probe-load", { id: "memory-save" });
      } catch {
        sourceComponentLoadable = false;
      }
      results.push({
        user_text: userText,
        runtime: {
          memory_access_enabled: flag("memory_access_enabled"),
          memory_update_enabled: flag("memory_update_enabled"),
        },
        runtime_text: runtimeText,
        registered_tools: registeredTools,
        source_component_listed: runtimeText.includes("`memory-save`"),
        source_component_loadable: sourceComponentLoadable,
        manual_skill_advertised: sentSystemPrompt.includes("manual-study"),
      });
    } finally {
      session.dispose();
    }
  }
  process.stdout.write(JSON.stringify(results));
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
