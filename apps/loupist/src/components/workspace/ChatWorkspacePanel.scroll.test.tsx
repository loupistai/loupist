import { render, screen, within } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { EVIDENCE_TAB_ID, WORK_TAB_ID, knowledgeEntry, pushEvidenceEntry, workWorkspaceTab } from "../../chat/workspace";
import { ChatWorkspacePanel } from "./ChatWorkspacePanel";
import { ChatMapProvider } from "./ChatMapContext";
import "../../styles/artifacts.css";
import "../../styles/memory.css";

const bridge = () => new MockLoupistBridge({ delay_ms: 0 });

const panelProps = (client: MockLoupistBridge) => ({
  bridge: client, loupistId: "loupist", threadId: "chat", open: true,
  session: { tabs: [], active_tab_id: undefined },
  theme: "light" as const, width: 420, height: 640, placement: "side" as const,
  onWidthChange: vi.fn(), onHeightChange: vi.fn(), onPlacementToggle: vi.fn(),
  onSelectTab: vi.fn(), onUpdateTab: vi.fn(), onCloseTab: vi.fn(), onOpenArtifact: vi.fn(), onAskArtifact: vi.fn(),
  onOpenKnowledge: vi.fn(), onOpenKnowledgePage: vi.fn(), onAskKnowledge: vi.fn(),
  onNewBrowser: vi.fn(), onOpenLink: vi.fn(), onClose: vi.fn(),
});

/** The tab bar and status row stay put. The body under them is the vertical scrollport. */
function expectScrollBody(panel: HTMLElement) {
  const body = panel.querySelector(".workspace-tab-body");
  expect(body).toBeInstanceOf(HTMLElement);
  const style = getComputedStyle(body!);
  expect(style.overflowY).toBe("auto");
  expect(Number.parseFloat(style.minHeight)).toBe(0);
  expect(style.flexGrow).toBe("1");
  expect(panel.querySelector(".workspace-tabbar")?.closest(".workspace-tab-body")).toBeNull();
  const status = panel.querySelector(".workspace-status");
  if (status) expect(status.closest(".workspace-tab-body")).toBeNull();
  return body!;
}

describe("ChatWorkspacePanel tab scroll", () => {
  it("scrolls a Lens page in Evidence while the status row stays outside the scrollport", async () => {
    const client = bridge();
    const evidence = pushEvidenceEntry(undefined, knowledgeEntry({ loupistId: "loupist", surface: "lens" }, "Lens"));
    render(<ChatWorkspacePanel {...panelProps(client)} session={{ tabs: [evidence], active_tab_id: EVIDENCE_TAB_ID }} />);
    const panel = screen.getByRole("complementary", { name: "Side panel" });
    const body = expectScrollBody(panel);
    const page = await within(panel).findByRole("button", { name: "Open in Lens" });
    const reader = page.closest(".workspace-page-reader");
    expect(reader).toBeInstanceOf(HTMLElement);
    expect(body.contains(reader)).toBe(true);
    const readerStyle = getComputedStyle(reader!);
    expect(readerStyle.overflowY).toBe("auto");
    expect(Number.parseFloat(readerStyle.minHeight)).toBe(0);
    expect(readerStyle.flexGrow).toBe("1");
    expect(getComputedStyle(reader!.parentElement!).display).toBe("flex");
    expect(getComputedStyle(reader!.parentElement!).flexDirection).toBe("column");
    expect(panel.querySelector(".workspace-status")).not.toBeNull();
  });

  it("scrolls the Results body under its status row", () => {
    const client = bridge();
    render(<ChatWorkspacePanel {...panelProps(client)} session={{
      tabs: [workWorkspaceTab({ artifact_id: "note", revision: 1, kind: "markdown", title: "Saved note", digest: "a".repeat(64) }, "chat")],
      active_tab_id: WORK_TAB_ID,
    }} />);
    const panel = screen.getByRole("complementary", { name: "Side panel" });
    const body = expectScrollBody(panel);
    expect(panel.querySelector(".workspace-work-tab")?.contains(body)).toBe(true);
    expect(body.contains(panel.querySelector(".workspace-status"))).toBe(false);
  });

  it("scrolls the World body under the tab bar", () => {
    const client = bridge();
    const props = panelProps(client);
    render(<ChatMapProvider bridge={client} loupistId="loupist" chatId="chat" messages={[]} onOpen={() => {}}><ChatWorkspacePanel {...props} /></ChatMapProvider>);
    const panel = screen.getByRole("complementary", { name: "Side panel" });
    const body = expectScrollBody(panel);
    expect(body.querySelector(".chat-mini-map")).not.toBeNull();
  });
});
