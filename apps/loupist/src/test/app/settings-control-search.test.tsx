import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { settingsSections } from "../../app/navigation";
import { openApp, mainTab } from "../renderApp";

it("finds Appearance controls from another section by either language or the description and focuses the control", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const refreshModels = vi.spyOn(bridge, "providerModels");
  await openApp(bridge);
  await user.click(await mainTab(/Settings/));
  const search = await screen.findByRole("searchbox", { name: "Search settings" });
  const nav = screen.getByRole("navigation", { name: "Settings sections" });
  refreshModels.mockClear();
  for (const query of ["Display currency", i18n.t("settings:appearance.displayCurrency.label", { lng: "ko" }), i18n.t("settings:appearance.displayCurrency.description")]) {
    fireEvent.change(search, { target: { value: query } });
    expect(await within(nav).findByRole("button", { name: "Display currency" })).toBeVisible();
  }
  expect(refreshModels).not.toHaveBeenCalled();
  await user.click(within(nav).getByRole("button", { name: "Display currency" }));
  await waitFor(() => expect(screen.getByRole("combobox", { name: "Display currency" })).toHaveFocus());
  fireEvent.change(search, { target: { value: "Maximum retries" } });
  await user.click(await within(nav).findByRole("button", { name: "Maximum retries" }));
  await waitFor(() => expect(screen.getByRole("spinbutton", { name: "Maximum retries" })).toHaveFocus());
});

it.each(settingsSections(true).map(item => item.id))("finds the labels displayed on the %s Settings page", async section => {
  const user = userEvent.setup();
  await openApp(new MockLoupistBridge({ delay_ms: 0, userSkillsAvailable: section === "skills" }));
  await user.click(await mainTab(/Settings/));
  await waitFor(() => expect(document.getElementById(`settings-section-${section}`)).toBeVisible());
  await user.click(document.querySelector<HTMLButtonElement>(`#settings-section-${section}`)!);
  const page = document.querySelector<HTMLElement>(".settings-page")!;
  const search = screen.getByRole("searchbox", { name: "Search settings" });
  const nav = screen.getByRole("navigation", { name: "Settings sections" });
  // Collect the names as a reader encounters them, independently of the search index.
  const labels = new Set<string>();
  await waitFor(() => {
    for (const node of page.querySelectorAll<HTMLElement>('label[for], [role="group"][aria-label], .settings-row strong, .skill-title, .marketplace-entry-title :is(h3,h4), tr td:first-child, button')) {
      if (node.closest('[hidden],[aria-hidden="true"],[role="dialog"]')) continue;
      const label = node.getAttribute("aria-label") || node.querySelector("strong,.skill-title")?.textContent || node.textContent;
      if (label?.trim()) labels.add(label.trim());
    }
    expect(labels.size).toBeGreaterThan(0);
  });
  for (const label of labels) {
    fireEvent.change(search, { target: { value: label } });
    expect(Array.from(nav.querySelectorAll("[data-setting-result]"), node => node.getAttribute("aria-label"))).toContain(label);
  }
});

it("omits user Skills from Settings navigation and search when an older bridge omits availability", async () => {
  const user = userEvent.setup();
  await openApp();
  await user.click(await mainTab(/Settings/));
  const nav = screen.getByRole("navigation", { name: "Settings sections" });
  expect(within(nav).getByRole("button", { name: "General" })).toHaveAttribute("aria-current", "page");
  expect(within(nav).queryByRole("button", { name: "Skills" })).toBeNull();
  await user.type(screen.getByRole("searchbox", { name: "Search settings" }), "Skill Creator");
  expect(await within(nav).findByRole("status")).toHaveTextContent("No settings found");
  expect(document.querySelector(".skills-settings")).toBeNull();
});

it("redirects a stale Skills selection to General when availability is missing", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0, userSkillsAvailable: true });
  await openApp(bridge);
  await user.click(await mainTab(/Settings/));
  const nav = screen.getByRole("navigation", { name: "Settings sections" });
  await user.click(await within(nav).findByRole("button", { name: "Skills" }));
  await screen.findByRole("switch", { name: /Skill Creator/ });
  delete bridge.extensionFlags.userSkillsAvailable;
  window.dispatchEvent(new Event("focus"));
  await waitFor(() => expect(within(nav).getByRole("button", { name: "General" })).toHaveAttribute("aria-current", "page"));
  expect(within(nav).queryByRole("button", { name: "Skills" })).toBeNull();
  expect(document.querySelector(".skills-settings")).toBeNull();
});
