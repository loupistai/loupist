import { act, fireEvent, render, screen } from "@testing-library/react";
import i18n from "../../i18n";
import { ChatConversation } from "./ChatConversation";
import { WorldProcessingStatus } from "./WorldProcessingStatus";
import { evidencePresentation } from "../../chat/evidence";
import type { ChatEvidenceRef, LoupistBridge } from "../../types";
import { AppHeader } from "../app/AppHeader";
import { SidebarProvider } from "../ui/sidebar";
import { MOCK_LOUPISTS } from "../../bridge/mock/fixtures";

afterEach(async () => {
  await act(async () => { await i18n.changeLanguage("en"); });
});

it.each(["world", "lens"] as const)("localizes the %s panel name in the app header", async kind => {
  render(<SidebarProvider><AppHeader runningLoupists={new Set()} unreadCounts={{}} loupists={[MOCK_LOUPISTS[0]!]} loupist={MOCK_LOUPISTS[0]!}
    tab="chat" markets={[]} workspaceOpen={false} workspaceKind={kind} workspacePlacement="side"
    onSelect={vi.fn()} onCreate={vi.fn()} onNavigate={vi.fn()} onLoupistSettings={vi.fn()} onWorkspaceKind={vi.fn()} onWorkspaceSplit={vi.fn()} onWorkspaceToggle={vi.fn()} /></SidebarProvider>);
  await act(async () => { await i18n.changeLanguage("en-XA"); });
  expect(screen.getByTestId("header-panel-toggle")).toHaveTextContent(i18n.t(`chat:workspaceEvidence.kind.${kind}`));
});

it("switches the morning message language without changing the Loupist name", async () => {
  const suggest = vi.fn();
  const bridge = {
    knowledgeMap: async () => ({ nodes: [], edges: [] }),
    knowledgeGrowthEvents: async () => ({ events: [], nextCursor: null }),
    inboxList: async () => ({ items: [], nextCursor: null }),
    journalList: async () => [],
    watchlistList: async () => [],
  } as unknown as LoupistBridge;
  render(<ChatConversation home bridge={bridge} messages={[]} messageKeys={[]} loupistId="loupist" loupistName="My research"
    onSuggestion={suggest} onReadAttachment={vi.fn()} />);
  expect(await screen.findByRole("button", { name: "What should I know this week?" })).toBeVisible();
  await act(async () => { await i18n.changeLanguage("en-XA"); });
  expect(screen.getByText("My research")).toBeVisible();
  const buttons = screen.getAllByRole("button");
  expect(buttons).toHaveLength(3);
  expect(screen.getByRole("article").querySelector(".morning-lead")?.textContent).toMatch(/^⟦.*⟧$/);
  for (const button of buttons) expect(button.textContent).toMatch(/^⟦.*⟧$/);
  fireEvent.click(buttons[0]!);
  expect(suggest).toHaveBeenCalledWith(expect.stringMatching(/^⟦.*⟧$/));
});

it("resolves status and evidence labels from the current language", async () => {
  render(<WorldProcessingStatus initialStatus="queued" />);
  expect(screen.getByRole("status")).toHaveTextContent("World: queued");
  const evidence = { kind: "original", tool_name: "web_fetch" } as ChatEvidenceRef;
  expect(evidencePresentation(evidence).label).toBe("Retrieved source");
  await act(async () => { await i18n.changeLanguage("en-XA"); });
  expect(screen.getByRole("status").textContent).toMatch(/^⟦.*⟧$/);
  expect(evidencePresentation(evidence).label).toMatch(/^⟦.*⟧$/);
});
