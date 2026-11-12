import i18n from "../i18n";
import type { LoupistAvailability } from "../contracts/loupist";
import { prepareFixture, previewKo, type PreviewBridge } from "./fixtures";
import type { PreviewScenario } from "./scenarios";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string) => Element | null;
  navigate: (root: HTMLElement, tab: "map" | "lens" | "schedule" | "settings") => Promise<void>;
  selectPanel: (kind: "lens" | "world" | "materials") => Promise<void>;
  click: (selector: string, root?: ParentNode) => Promise<void>;
  clickMapAction: (selector: string, root?: ParentNode) => Promise<void>;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
};

export function readerScenarios({ base, select, navigate, selectPanel, click, clickMapAction, until }: Helpers): PreviewScenario[] {
  return [
    ...(["text", "file", "link"] as const).map((kind): PreviewScenario => base(`source-add-${kind}`, `Add a source: ${kind}`, () => select(`.source-add-dialog [role="tabpanel"][data-state="active"]`), async root => {
      await navigate(root, "map");
      await clickMapAction("[data-map-library]", root);
      await click(".source-library-heading .library-add");
      const dialog = await until(() => select(".source-add-dialog"), "Add source did not open");
      if (kind !== "text") await click(`[role="tab"][id$="-trigger-${kind}"]`, dialog);
    })),
    ...(["list", "csv", "markdown", "empty", "error"] as const).map((kind): PreviewScenario => ({
      ...base(`workspace-files-${kind}`, `Workspace files: ${kind}`, () => select(kind === "csv" ? ".reader-csv-preview" : kind === "markdown" ? ".workspace-file-content [data-streamdown]" : kind === "error" ? '.workspace-files [role="alert"]' : kind === "empty" ? ".workspace-file-list .reader-empty" : ".workspace-file-entry"), async () => {
        await until(() => select(".header-panel-toggle"), "Chat did not load");
        await selectPanel("materials");
        await click(".workspace-add > summary");
        await click(".workspace-add-menu button:first-child");
        await until(() => !document.querySelector<HTMLDetailsElement>(".workspace-add")?.open, "Workspace menu did not close after opening Files");
        if (kind === "csv" || kind === "markdown") await click(`[data-file-path="${kind === "csv" ? "quarterly.csv" : "research.md"}"]`);
      }),
      setup: async bridge => {
        await prepareFixture(bridge);
        bridge.workspaceList = async () => {
          if (kind === "error") throw new Error(i18n.t("chat:workspaceFiles.couldNotReadThisWorkspaceItem"));
          return { path: "", truncated: false, entries: kind === "empty" ? [] : [
            { kind: "directory", name: "reports", path: "reports", size_bytes: null },
            { kind: "file", name: "quarterly.csv", path: "quarterly.csv", size_bytes: 140 },
            { kind: "file", name: "research.md", path: "research.md", size_bytes: 240 },
          ] };
        };
        bridge.workspaceRead = async request => ({ path: request.path, content: kind === "csv" ? "period,revenue,margin\n2026 Q1,125000000,0.148\n2026 Q2,142000000,0.153\n2026 Q3,156000000,0.162" : previewKo() ? "# 분기 실적 검토\n\n## 확인할 내용\n\n매출과 이익률을 같은 기간의 보관한 자료와 비교해요.\n\n- 보고 기간을 확인해요.\n- 현금흐름과 가동률을 비교해요." : "# Quarterly research\n\n## What to check\n\nCompare revenue and margins against retained sources for the same period.\n\n- Confirm the reporting period.\n- Compare cash conversion and utilization.", content_sha256: "a".repeat(64), media_type: kind === "csv" ? "text/csv" : "text/markdown", size_bytes: 240, binary: false, truncated: false });
      },
    })),
    ...(["workspace_unavailable", "earlier_version", "newer_version"] as const).map((status): PreviewScenario => ({
      ...base(`loupist-${status.replaceAll("_", "-")}`, `Loupist folder: ${status}`, () => select(status === "workspace_unavailable" ? '.app-selection-error[role="status"]' : '.app-stage [role="alert"]')),
      setup: async (bridge: PreviewBridge) => {
        await prepareFixture(bridge);
        const availability: LoupistAvailability = { status };
        const list = bridge.loupistList.bind(bridge);
        bridge.loupistList = async () => (await list()).map(item => ({ ...item, availability }));
        const read = bridge.loupistSelect.bind(bridge);
        bridge.loupistSelect = async id => { const workspace = await read(id); return { ...workspace, loupist: { ...workspace.loupist, availability } }; };
      },
    })),
  ];
}
