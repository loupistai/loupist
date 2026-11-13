import { useState } from "react";
import i18n from "../../i18n";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ConfigScope, LoupistSummary } from "../../types";
import { SkillsSettings } from "./SkillsSettings";
function View({ bridge, loupists }: { bridge: MockLoupistBridge; loupists: LoupistSummary[] }) {
  const [scope, setScope] = useState<ConfigScope>({ kind: "global" });
  return <SkillsSettings bridge={bridge} loupists={loupists} scope={scope} onScopeChange={setScope} />;
}
async function setup() { const bridge = new MockLoupistBridge({ delay_ms: 0 }); const loupists = await bridge.loupistList(); render(<View bridge={bridge} loupists={loupists} />); await screen.findByRole("switch", { name: "Skill Creator: disabled" }); return { bridge, user: userEvent.setup() }; }

describe("Skills management", () => {
  it("replaces loading with an error and recovers through Reload", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const read = vi.spyOn(bridge, "skillsList").mockRejectedValueOnce(new Error("Could not load skills."));
    render(<View bridge={bridge} loupists={await bridge.loupistList()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load skills.");
    expect(screen.queryByText(i18n.t("settings:skillsSettings.loadingSkills"))).not.toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: i18n.t("settings:skillsSettings.reloadSkills") }));
    await screen.findByRole("switch", { name: "Skill Creator: disabled" });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(read).toHaveBeenCalledTimes(2);
  });
  it("shows included skills, inherits optional preferences, and restores one local override", async () => {
    const { bridge, user } = await setup();
    expect(screen.queryByRole("switch", { name: /World:/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("switch", { name: "Skill Creator: disabled" }));
    await screen.findByRole("switch", { name: "Skill Creator: enabled" });
    await user.click(screen.getByRole("button", { name: "Reset Skill Creator" }));
    await screen.findByRole("switch", { name: "Skill Creator: disabled" });
    expect((await bridge.configRead({ scope: { kind: "global" } })).explicit?.skills?.["skill-creator"]?.enabled).toBeUndefined();
    await user.click(screen.getByRole("switch", { name: "Valuation: enabled" }));
    await screen.findByRole("switch", { name: "Valuation: disabled" });
    await user.selectOptions(screen.getByLabelText("Available to"), "loupist-quality");
    await user.click(await screen.findByRole("switch", { name: "Valuation: disabled" }));
    await screen.findByRole("switch", { name: "Valuation: enabled" });
    const local = await bridge.configRead({ scope: { kind: "project", loupistId: "loupist-quality" } });
    expect(local.explicit?.skills).toEqual({ valuation: { enabled: true } });
    await user.click(screen.getByRole("button", { name: "Reset Valuation" }));
    await screen.findByRole("switch", { name: "Valuation: disabled" });
    expect((await bridge.agentSkillCatalog("loupist-quality")).find((s) => s.id === "valuation")?.enabled).toBe(false);
  });
  it("creates, reads, edits, and deletes a scoped workflow without changing Chats", async () => {
    const { bridge, user } = await setup();
    const prior = (await bridge.loupistSelect("loupist-quality")).threads;
    await user.selectOptions(screen.getByLabelText("Available to"), "loupist-quality");
    await screen.findByRole("switch", { name: "Skill Creator: disabled" });
    await user.click(screen.getByRole("button", { name: "Create skill" }));
    const create = screen.getByRole("dialog", { name: "Create a skill" });
    await user.type(within(create).getByLabelText("Name"), "Revenue Review");
    await user.type(within(create).getByLabelText("When to use it"), "Review quarterly revenue.");
    await user.type(within(create).getByLabelText("Instructions"), "Compare like periods and cite inputs.");
    await user.click(within(create).getByRole("button", { name: "Save skill" }));
    await user.click(await screen.findByRole("button", { name: /Revenue Review.*Review quarterly revenue/ }));
    const detail = await screen.findByRole("dialog", { name: "Revenue Review" });
    expect(within(detail).getByText("Compare like periods and cite inputs.")).toBeVisible();
    await user.click(within(detail).getByRole("button", { name: "Edit instructions" }));
    const edit = await screen.findByRole("dialog", { name: "Edit Revenue Review" });
    await user.clear(within(edit).getByLabelText("Instructions"));
    await user.type(within(edit).getByLabelText("Instructions"), "Use consolidated figures and show the period.");
    await user.click(within(edit).getByRole("button", { name: "Save skill" }));
    await screen.findByRole("switch", { name: "Revenue Review: enabled" });
    expect((await bridge.skillsList({ scope: { kind: "global" } })).skills.some((s) => s.id === "revenue-review")).toBe(false);
    const entry = (await bridge.skillsList({ scope: { kind: "project", loupistId: "loupist-quality" } })).skills.find((s) => s.id === "revenue-review")!;
    expect((await bridge.skillsRead({ scope: { kind: "project", loupistId: "loupist-quality" }, key: entry.path! })).instructions).toContain("consolidated");
    await user.click(screen.getByRole("button", { name: /Revenue Review.*Review quarterly revenue/ }));
    await user.click(await within(await screen.findByRole("dialog", { name: "Revenue Review" })).findByRole("button", { name: "Delete skill" }));
    await user.click(within(screen.getByRole("dialog", { name: "Delete Revenue Review?" })).getByRole("button", { name: "Delete skill" }));
    await waitFor(() => expect(screen.queryByRole("switch", { name: "Revenue Review: enabled" })).not.toBeInTheDocument());
    expect((await bridge.loupistSelect("loupist-quality")).threads).toEqual(prior);
  });
  it("keeps a failed GitHub import open with the entered source", async () => {
    const { bridge, user } = await setup();
    vi.spyOn(bridge, "skillsImport").mockRejectedValue(new Error("The public GitHub download is unavailable."));
    await user.click(screen.getByRole("button", { name: "Add skill" }));
    const dialog = screen.getByRole("dialog", { name: "Add a skill" });
    const url = "https://github.com/example/workflows/tree/main/skills/review";
    await user.type(within(dialog).getByLabelText("GitHub folder URL"), url);
    await user.click(within(dialog).getByRole("button", { name: "Add skill" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("download is unavailable");
    expect(within(dialog).getByLabelText("GitHub folder URL")).toHaveValue(url);
  });
});

it("translates bundled Skill metadata while preserving personal Skill content", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const scope = { kind: "global" } as const;
  await bridge.skillsCreate({ scope, name: "My research", description: "Keep this description in English.", instructions: "User instructions stay unchanged." });
  render(<View bridge={bridge} loupists={await bridge.loupistList()} />);
  await screen.findByRole("switch", { name: "Skill Creator: disabled" });
  try {
    await act(async () => { await i18n.changeLanguage("en-XA"); });
    expect(screen.getByText(i18n.t("settings:bundledSkills.research.name")).textContent).toMatch(/^⟦.*⟧$/);
    expect(screen.getByText("My research")).toBeVisible();
    expect(screen.getByText("Keep this description in English.")).toBeVisible();
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});

it("lists a bundled Skill by its short line for people and keeps the full description for its detail", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  render(<View bridge={bridge} loupists={await bridge.loupistList()} />);
  await screen.findByRole("switch", { name: "Skill Creator: disabled" });
  const blurb = i18n.t("chat:skillBlurb.dividend-income");
  const full = i18n.t("settings:bundledSkills.dividend-income.description");
  expect(blurb).not.toBe(full);
  expect(screen.getByText(blurb)).toBeVisible();
  expect(screen.queryByText(full)).toBeNull();
});

it("shows a bundled Skill's description once, its instructions, and its files behind the shared disclosure", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  render(<View bridge={bridge} loupists={await bridge.loupistList()} />);
  await screen.findByRole("switch", { name: "Skill Creator: disabled" });
  const user = userEvent.setup();
  await user.click(screen.getByText(i18n.t("chat:skillBlurb.dividend-income")));
  const dialog = await screen.findByRole("dialog");
  const full = i18n.t("settings:bundledSkills.dividend-income.description");
  const instructions = await waitFor(() => { const pre = dialog.querySelector(".skill-instructions")!; expect(pre.textContent).toMatch(/^# Dividend income/); return pre; });
  expect(instructions.textContent).not.toContain(full);
  expect(within(dialog).getAllByText(full)).toHaveLength(1);
  const files = dialog.querySelector("details")!;
  expect(files).toHaveClass("ui-disclosure");
  await user.click(within(files).getByText(i18n.t("settings:skillsSettings.includedFilesAndLocation")));
  expect(within(files).getByText("SKILL.md")).toBeVisible();
});
