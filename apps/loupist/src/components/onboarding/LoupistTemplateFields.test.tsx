import { useState } from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { LoupistTemplateFields, existingDraft, templateDraft, type LoupistDraft } from "./LoupistTemplateFields";
import type { LoupistSummary } from "@/types";

function Harness({ onDraft }: { onDraft?: (draft: LoupistDraft) => void }) {
  const [draft, setDraft] = useState<LoupistDraft>(() => templateDraft("portfolio"));
  return <LoupistTemplateFields draft={draft} onChange={next => { setDraft(next); onDraft?.(next); }} />;
}

it("offers eight templates in order, with no icons on the cards", () => {
  render(<Harness />);
  const cards = within(screen.getByRole("group", { name: /template/i })).getAllByRole("button");
  expect(cards.map(card => card.querySelector("strong")?.textContent)).toEqual([
    "My portfolio", "Semiconductors", "Dividend income", "US big tech", "Crypto", "Macro & rates", "Commodities", "Custom",
  ]);
  expect(cards.every(card => !card.querySelector("svg"))).toBe(true);
});

it("names each card with its description and repeats the chosen one's in the preview, which a short window's title-only cards rely on", async () => {
  const user = userEvent.setup();
  const { container } = render(<Harness />);
  expect(screen.getByRole("button", { name: "Semiconductors Chips, memory, foundries and equipment" })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /^Semiconductors/ }));
  expect(container.querySelector(".loupist-identity-preview small")).toHaveTextContent("Chips, memory, foundries and equipment");
});

it("fills a class template's persona and picks starters into the draft, never following them itself", async () => {
  const user = userEvent.setup();
  const drafts: LoupistDraft[] = [];
  render(<Harness onDraft={draft => drafts.push(draft)} />);
  expect(screen.queryByText("Also follow:")).toBeNull();
  await user.click(screen.getByRole("button", { name: /^Macro & rates/ }));
  expect(screen.getByLabelText("Name")).toHaveValue("Macro Loupist");
  expect(screen.getByLabelText("Focus")).toHaveValue("Interest rates, inflation, currencies and central banks");
  expect(screen.getByText("Also follow:")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Follow US 10-year" }));
  expect(screen.getByRole("button", { name: "Follow US 10-year" })).toHaveAttribute("aria-pressed", "true");
  expect(drafts.at(-1)?.starters).toEqual(["US 10-year Treasury"]);
  // A second tap drops the pick.
  await user.click(screen.getByRole("button", { name: "Follow US 10-year" }));
  expect(screen.getByRole("button", { name: "Follow US 10-year" })).toHaveAttribute("aria-pressed", "false");
  expect(drafts.at(-1)?.starters).toEqual([]);
  await user.click(screen.getByRole("button", { name: "Follow EUR/USD" }));
  // Another template starts with no picks.
  await user.click(screen.getByRole("button", { name: /^Crypto/ }));
  expect(drafts.at(-1)?.starters).toEqual([]);
  expect(screen.getAllByRole("button", { name: /^Follow / }).map(chip => chip.textContent)).toEqual(["Bitcoin", "Ethereum", "Solana", "USD Coin"]);
  expect(screen.getAllByRole("button", { name: /^Follow / }).every(chip => chip.getAttribute("aria-pressed") === "false")).toBe(true);
  await user.click(screen.getByRole("button", { name: /^Custom/ }));
  expect(screen.queryByRole("button", { name: /^Follow / })).toBeNull();
});

it("keeps onboarding's template step to templates and starters, plus Focus for Custom", async () => {
  const user = userEvent.setup();
  function Essentials() {
    const [draft, setDraft] = useState<LoupistDraft>(() => templateDraft("portfolio"));
    return <LoupistTemplateFields draft={draft} onChange={setDraft} essentials />;
  }
  render(<Essentials />);
  // The name and face have their own step.
  expect(screen.queryByLabelText("Name")).toBeNull();
  expect(screen.queryByLabelText("Focus")).toBeNull();
  expect(screen.queryByRole("group", { name: "Face color" })).toBeNull();
  expect(screen.queryAllByRole("radio")).toHaveLength(0);
  expect(screen.getByText("You can change its focus and tone later in Loupist settings.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: /^Custom/ }));
  expect(screen.getByLabelText("Focus")).toBeVisible();
});

it("suggests a template's name for a Loupist the host named after its folder, and keeps a chosen one", () => {
  const seeded = { id: "loupist-seed", name: "workspace", philosophy: "", instructions: "", avatar: "initial", tone: "balanced", workspace_path: "/tmp/e2e/workspace", record_count: 0, updated_at: "", accent: "", enabled_skill_ids: [], last_model_profile_id: null, availability: { status: "available" } } as unknown as LoupistSummary;
  expect(existingDraft(seeded).name).toBe("Portfolio Loupist");
  expect(existingDraft({ ...seeded, template: "macro" }).name).toBe("Macro Loupist");
  expect(existingDraft({ ...seeded, name: "Rates desk" }).name).toBe("Rates desk");
});
