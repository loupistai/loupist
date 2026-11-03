import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { WorkspaceTabBar } from "./WorkspaceTabBar";
import type { ChatWorkspaceSession, ChatWorkspaceTab } from "../../chat/workspace";

const tabs: ChatWorkspaceTab[] = [
  { id: "one", kind: "browser", url: "", title: "First page", loading: false },
  { id: "two", kind: "browser", url: "", title: "Second page", loading: false },
];

function TabBar({ onNewBrowser = vi.fn() }) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [session, setSession] = useState<ChatWorkspaceSession>({ tabs, active_tab_id: "one" });
  return <><WorkspaceTabBar
    session={session} activeTab={session.tabs.find((tab) => tab.id === session.active_tab_id)}
    menuOpen={menuOpen}
    onMenuOpenChange={setMenuOpen} onSelectTab={(id) => setSession((current) => ({ ...current, active_tab_id: id }))}
    onCloseTab={(tab) => setSession((current) => ({ tabs: current.tabs.filter((item) => item.id !== tab.id), active_tab_id: current.tabs.find((item) => item.id !== tab.id)?.id }))}
    onNewBrowser={onNewBrowser}
  /></>;
}

describe("WorkspaceTabBar", () => {
  it("opens items with the keyboard and dismisses with Escape", async () => {
    const user = userEvent.setup();
    const onNewBrowser = vi.fn();
    render(<TabBar onNewBrowser={onNewBrowser} />);
    const trigger = screen.getByLabelText("Open workspace item");
    trigger.focus();
    await user.keyboard("{ArrowDown}");
    await waitFor(() => expect(screen.getByRole("menuitem", { name: /New browser tab/ })).toHaveFocus());
    await user.keyboard("{Escape}");
    expect(trigger).toHaveFocus();
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    await user.keyboard("{ArrowDown}{Enter}");
    expect(onNewBrowser).toHaveBeenCalledOnce();
  });

  it("moves between tabs and keeps focus after closing one with Delete", async () => {
    const user = userEvent.setup();
    render(<TabBar />);
    screen.getByRole("tab", { name: "First page" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Second page" })).toHaveFocus();
    await user.keyboard("{Delete}");
    expect(screen.queryByRole("tab", { name: "Second page" })).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "First page" })).toHaveFocus();
    await user.keyboard("{Delete}");
    // Evidence is a fixed tab: it takes focus and cannot be closed.
    const evidence = screen.getByRole("tab", { name: "Evidence" });
    expect(evidence).toHaveFocus();
    await user.keyboard("{Delete}");
    expect(evidence).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close Evidence tab" })).toBeNull();
  });

  it("puts Evidence and Work first and marks a tab that received content instead of switching to it", () => {
    render(<WorkspaceTabBar session={{ tabs, active_tab_id: "one", badges: ["two"] }} activeTab={tabs[0]} menuOpen={false}
      onMenuOpenChange={vi.fn()} onSelectTab={vi.fn()} onCloseTab={vi.fn()} onNewBrowser={vi.fn()} />);
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["Evidence", "Work", "First page", "Second pageNew in Second page"]);
    expect(screen.getByRole("tab", { name: "First page" })).toHaveAttribute("aria-selected", "true");
    expect(document.querySelectorAll(".workspace-tab-badge")).toHaveLength(1);
  });
});
