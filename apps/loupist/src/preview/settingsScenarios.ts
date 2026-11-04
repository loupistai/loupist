import i18n from "../i18n";
import { prepareFixture, previewKo, type PreviewBridge } from "./fixtures";
import type { PreviewScenario } from "./scenarios";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string) => Element | null;
  settings: (root: HTMLElement, section: string) => Promise<void>;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
};

/** Settings destinations and secondary dialogs absent from the original preview catalog. */
export function settingsScenarios({ base, select, settings, until }: Helpers): PreviewScenario[] {
  const button = async (key: "settings:skillsSettings.createSkill" | "settings:skillsSettings.addSkill") => {
    const target = await until(() => [...document.querySelectorAll<HTMLButtonElement>(".skills-settings button")]
      .find(item => item.textContent?.trim() === i18n.t(key) && !item.disabled), `Missing settings action: ${key}`);
    target.click();
  };
  const skills = ["list", "detail", "create", "add", "add-zip", "add-folder", "empty", "error", "loading"] as const;
  return [
    ...(["chat", "learning", "capabilities", "workspace", "remove"] as const).map((section): PreviewScenario => ({
      ...base(`loupist-settings-${section}`, `Loupist preferences: ${section}`, () => select(section === "remove" ? ".loupist-delete-dialog" : `[data-loupist-section="${section}"]`), async () => {
        const open = await until(() => select("[data-header-loupist-settings]") as HTMLButtonElement | null, "Loupist header did not load");
        open.click();
        if (section === "remove") {
          const remove = await until(() => select(".agent-removal-card button") as HTMLButtonElement | null, "Remove action did not load");
          remove.click();
        } else {
          const summary = await until(() => select(`[data-loupist-section="${section}"] > summary`) as HTMLElement | null, "Loupist section did not load");
          summary.click();
          summary.scrollIntoView?.({ block: "start" });
          await until(() => select(`[data-loupist-section="${section}"][open]`), "Loupist section did not open");
        }
      }), setup: prepareFixture,
    })),
    base("settings-appearance", "Theme and language preferences", () => select(".appearance-card"), root => settings(root, "appearance")),
    base("settings-agent-advanced", "Chat preferences, context limits and recovery", () => select(".agent-config-advanced[open]"), async root => {
      await settings(root, "agent");
      const summary = await until(() => select(".agent-config-advanced summary") as HTMLElement | null, "Advanced settings did not load");
      summary.click();
      summary.scrollIntoView?.({ block: "start" });
    }),
    ...skills.map((kind): PreviewScenario => ({
      ...base(`settings-skills-${kind}`, `Skills: ${kind}`, () => select(kind === "list" ? ".skills-list" : kind === "detail" ? ".skill-instructions" : kind === "create" ? ".skill-instructions-editor" : kind.startsWith("add") ? '[role="dialog"] select' : kind === "empty" ? ".skills-empty" : kind === "error" ? '.skills-settings [role="alert"]' : ".skills-settings"), async root => {
        await settings(root, "skills");
        if (kind === "detail") (await until(() => select(".skill-summary") as HTMLButtonElement | null, "Skills did not load")).click();
        if (kind === "create") await button("settings:skillsSettings.createSkill");
        if (kind.startsWith("add")) {
          await button("settings:skillsSettings.addSkill");
          if (kind !== "add") {
            const field = await until(() => select('[role="dialog"] select') as HTMLSelectElement | null, "Import choices did not load");
            field.value = kind === "add-zip" ? "zip" : "folder";
            field.dispatchEvent(new Event("change", { bubbles: true }));
          }
        }
      }),
      setup: async bridge => {
        await prepareFixture(bridge);
        if (kind === "loading") bridge.skillsList = () => new Promise(() => {});
        if (kind === "error") bridge.skillsList = async () => { throw new Error(i18n.t("settings:skillsSettings.couldNotLoadSkills")); };
        if (kind === "empty") {
          const read = bridge.skillsList.bind(bridge);
          bridge.skillsList = async request => ({ ...await read(request), skills: [] });
        }
      },
    })),
    ...(["edit", "delete"] as const).map((kind): PreviewScenario => ({
      ...base(`settings-skills-${kind}`, `Personal Skill: ${kind}`, () => select(kind === "edit" ? ".skill-instructions-editor" : '[role="dialog"]'), async root => {
        await settings(root, "skills");
        const skill = await until(() => [...document.querySelectorAll<HTMLButtonElement>(".skill-summary")].find(item => item.textContent?.includes("Quarterly Review")), "Personal Skill did not load");
        skill.click();
        const label = i18n.t(kind === "edit" ? "settings:skillsSettings.editInstructions" : "settings:skillsSettings.deleteSkill");
        const action = await until(() => [...document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')].find(item => item.textContent?.trim() === label), "Skill action did not load");
        action.click();
        await until(() => kind === "edit" ? select(".skill-instructions-editor") : document.querySelector('[role="dialog"]')?.textContent?.includes(i18n.t("settings:skillsSettings.deleteTitle", { name: "Quarterly Review" })), "Skill dialog did not open");
      }), setup: async bridge => {
        await prepareFixture(bridge);
        await bridge.skillsCreate({ scope: { kind: "global" }, name: "Quarterly Review", description: "Compare quarterly revenue and margins against retained primary sources.", instructions: "Identify the reporting period.\nCompare consolidated figures.\nLink every reported value to its source.\n".repeat(12) });
      },
    })),
    ...(["current", "available", "checking", "confirming", "downloading", "installing", "restarting", "blocked", "error", "unsupported"] as const).map((kind): PreviewScenario => ({
      ...base(`settings-updates-${kind}`, `Updates: ${kind}`, () => select(".update-card"), root => settings(root, "updates")),
      setup: async (bridge: PreviewBridge) => {
        await prepareFixture(bridge);
        const read = bridge.updateStatus.bind(bridge);
        bridge.updateStatus = async () => {
          const state = await read();
          return { ...state, supported: kind !== "unsupported", phase: kind === "checking" || kind === "confirming" || kind === "downloading" || kind === "installing" || kind === "restarting" ? kind : "idle",
            offer: kind !== "current" && kind !== "checking" && kind !== "error" && kind !== "unsupported" ? { offer_id: "preview-update", version: "0.0.2", published_at: "2026-11-04T00:00:00Z", notes: previewKo() ? "긴 문서와 여러 연구 방법을 더 편하게 읽을 수 있도록 화면 배치를 개선했어요." : "Improved reading layouts for long documents and collections of research methods." } : null,
            downloaded_bytes: 50, total_bytes: 100,
            error: kind === "error" ? i18n.t("settings:useAppUpdate.couldNotCheckForUpdates") : null,
            blockers: kind === "blocked" ? [{ id: "preview-chat", kind: "chat", label: previewKo() ? "한울정밀의 분기 실적을 검토하고 있어요." : "Reviewing Hanul Precision's quarterly results." }] : [] };
        };
      },
    })),
  ];
}
