import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { ShareSheet } from "./ShareSheet";

async function fixture(configure?: (bridge: MockLoupistBridge) => void) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  configure?.(bridge);
  render(<ShareSheet bridge={bridge} loupistId="loupist-quality" loupistName="Quality Compounder" scope={{ kind: "loupist" }} open onOpenChange={() => {}} />);
  const dialog = await screen.findByRole("dialog", { name: "Share Quality Compounder" });
  await within(dialog).findByRole("switch", { name: "Methods" });
  await waitFor(() => expect(within(dialog).getByRole("switch", { name: "Methods" })).toBeEnabled());
  return { bridge, dialog, user: userEvent.setup() };
}

it("keeps every output disabled until a blocking finding is left out, and offers It's fine only for personal details", async () => {
  const { bridge, dialog, user } = await fixture(bridge => bridge.sharing.setFindings());
  const exported = vi.spyOn(bridge, "shareExport");
  const findings = within(dialog).getByRole("region", { name: "Check before sharing (2)" });
  const account = within(findings).getByText(/looks like an account or card number/).closest("li")!;
  const personal = within(findings).getByText(/contains an email, phone number or your name/).closest("li")!;
  // The finding names the record's kind and title, never the matched value.
  expect(account).toHaveTextContent("Rule “Trim when a position passes 12% of the account” contains what looks like an account or card number.");
  expect(personal).toHaveTextContent("Method “Owner earnings” contains");
  expect(within(account).queryByRole("button", { name: "It's fine" })).toBeNull();
  expect(within(personal).getByRole("button", { name: "It's fine" })).toBeVisible();
  // Disabled outputs say why, under them.
  for (const name of ["Publish to GitHub", "Save as folder", "Save as ZIP"]) {
    expect(within(dialog).getByRole("button", { name })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name })).toHaveAccessibleDescription("Leave out the flagged items first.");
  }

  // A personal warning alone does not hold the outputs back; "It's fine" acknowledges it.
  await user.click(within(personal).getByRole("button", { name: "It's fine" }));
  expect(within(dialog).getByRole("button", { name: "Save as folder" })).toBeDisabled();
  await user.click(within(account).getByRole("button", { name: "Leave it out" }));
  expect(within(dialog).queryByRole("region", { name: /Check before sharing/ })).toBeNull();
  expect(within(dialog).queryByText("Leave out the flagged items first.")).toBeNull();
  const save = within(dialog).getByRole("button", { name: "Save as folder" });
  expect(save).toBeEnabled();

  await user.click(within(dialog).getByRole("switch", { name: "Rules" }));
  await user.selectOptions(within(dialog).getByRole("combobox", { name: "License" }), "CC0-1.0");
  await user.type(within(dialog).getByRole("textbox", { name: "Name to show (optional)" }), "Kim J.");
  await user.click(save);
  await waitFor(() => expect(exported).toHaveBeenCalledWith({
    loupistId: "loupist-quality", scope: { kind: "loupist" }, destination: "folder", license: "CC0-1.0", byline: "Kim J.",
    excludedItemKeys: ["p-trim-when"], acknowledgedFindingIds: ["f-personal"],
    sections: { profile: true, perspective: true, methods: true, principles: false, map: true },
  }));
  expect(await within(dialog).findByText("Saved to /Users/kimj/Desktop/quality-compounder")).toBeVisible();
});

it("replaces Publish with the not-available note when this build has no GitHub client", async () => {
  const { dialog } = await fixture(bridge => { bridge.sharing.account = { clientConfigured: false, indexConfigured: false, galleryTermsUrl: null, login: null }; });
  expect(within(dialog).getByText(/GitHub publishing isn't available in this version/)).toBeVisible();
  expect(within(dialog).queryByRole("button", { name: "Publish to GitHub" })).toBeNull();
  expect(within(dialog).getByRole("button", { name: "Save as folder" })).toBeEnabled();
  expect(within(dialog).getByRole("button", { name: "Save as ZIP" })).toBeEnabled();
});

it("signs in with a device code before the first publish, then publishes to the named repository", async () => {
  // The mock approves the code a few ticks after showing it.
  const { bridge, dialog, user } = await fixture(bridge => bridge.setStreamDelay(30));
  const publish = vi.spyOn(bridge, "sharePublish");
  await user.click(within(dialog).getByRole("button", { name: "Publish to GitHub" }));
  const signIn = await screen.findByRole("dialog", { name: "Sign in to GitHub" }, { timeout: 3000 });
  expect(within(signIn).getByLabelText("W D J B - M J H T")).toHaveTextContent("WDJB-MJHT");
  const repo = await screen.findByRole("textbox", { name: "Repository name" }, { timeout: 3000 });
  expect(repo).toHaveValue("loupist-quality-compounder");
  expect(screen.getByText(/Signed in to GitHub as @kimj/)).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Publish to GitHub" }));
  expect(await screen.findByText("Published to github.com/kimj/loupist-quality-compounder")).toBeVisible();
  expect(publish).toHaveBeenCalledWith(expect.objectContaining({ target: { repo: { name: "loupist-quality-compounder" } } }), expect.any(Function));
  // The pressed button is gone; focus lands on the step's title, not on the sheet.
  expect(screen.getByRole("heading", { name: "Share Quality Compounder" })).toHaveFocus();
});

it("lets the update step publish to a new repository or go back to the options", async () => {
  const { dialog, user } = await fixture(bridge => {
    bridge.sharing.account = { ...bridge.sharing.account, login: "kimj" };
    bridge.sharing.summaryOverrides = { repo: { fullName: "kimj/loupist-quality-compounder", url: "https://github.com/kimj/loupist-quality-compounder", version: 4 }, nextVersion: 5 };
  });
  await user.click(within(dialog).getByRole("button", { name: "Publish to GitHub" }));
  await user.click(await screen.findByRole("button", { name: "Publish as a new repository" }));
  expect(screen.getByRole("textbox", { name: /^Repository name/ })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Back" }));
  expect(await screen.findByRole("switch", { name: "Profile" })).toBeVisible();
});

it("refuses Update when the recorded repository now holds something else and offers a new one", async () => {
  const { bridge, dialog, user } = await fixture(bridge => {
    bridge.sharing.account = { ...bridge.sharing.account, login: "kimj" };
    bridge.sharing.summaryOverrides = { repo: { fullName: "kimj/loupist-quality-compounder", url: "https://github.com/kimj/loupist-quality-compounder", version: 3 }, nextVersion: 4 };
    bridge.sharing.updateRefusal = "repo_mismatch";
  });
  const publish = vi.spyOn(bridge, "sharePublish");
  await user.click(within(dialog).getByRole("button", { name: "Publish to GitHub" }));
  await user.click(await screen.findByRole("button", { name: /Update github\.com\/kimj\/loupist-quality-compounder/ }));
  expect(await screen.findByText("That repository now holds something else. Publish as a new repository.")).toBeVisible();
  expect(screen.getByRole("textbox", { name: /^Repository name/ })).toBeVisible();
  expect(publish).toHaveBeenCalledWith(expect.objectContaining({ target: { update: true } }), expect.any(Function));
});

it("keeps the Loupist's name out of the default repository name while Profile is off", async () => {
  const { dialog, user } = await fixture(bridge => { bridge.sharing.account = { ...bridge.sharing.account, login: "kimj" }; });
  await user.click(within(dialog).getByRole("switch", { name: "Profile" }));
  await user.click(within(dialog).getByRole("button", { name: "Publish to GitHub" }));
  expect(await screen.findByRole("textbox", { name: /^Repository name/ })).toHaveValue("loupist-shared");
});

it("offers only the licenses Rust allows, starts from its default and says what each one allows", async () => {
  const { bridge, dialog, user } = await fixture();
  const exported = vi.spyOn(bridge, "shareExport");
  const select = within(dialog).getByRole("combobox", { name: "License" });
  expect(within(select).getAllByRole("option").map(option => option.textContent)).toEqual(["CC BY-NC-SA 4.0", "CC BY 4.0", "CC BY-SA 4.0", "CC0 1.0", "CC BY-NC 4.0"]);
  expect(select).toHaveValue("CC-BY-NC-SA-4.0");
  expect(select).toHaveAccessibleDescription("Anyone can share and adapt it with credit, not for commercial use, and must keep the same license.");
  await user.selectOptions(select, "CC-BY-NC-4.0");
  expect(select).toHaveAccessibleDescription("Anyone can share and adapt it with credit, not for commercial use.");
  await user.click(within(dialog).getByRole("button", { name: "Save as ZIP" }));
  await waitFor(() => expect(exported).toHaveBeenCalledWith(expect.objectContaining({ license: "CC-BY-NC-4.0", destination: "zip" })));
});

it("links the chosen license's name to the legal text Rust names for it", async () => {
  const legal = { "CC-BY-NC-SA-4.0": "https://legal.example/by-nc-sa", "CC-BY-SA-4.0": "https://legal.example/by-sa" };
  const { bridge, dialog, user } = await fixture(bridge => {
    bridge.sharing.summaryOverrides = { allowedLicenses: [{ id: "CC-BY-NC-SA-4.0", url: legal["CC-BY-NC-SA-4.0"] }, { id: "CC-BY-SA-4.0", url: legal["CC-BY-SA-4.0"] }] };
  });
  const opened = vi.spyOn(bridge, "openExternalUrl").mockResolvedValue(undefined);
  await user.click(within(dialog).getByRole("button", { name: "CC BY-NC-SA 4.0" }));
  expect(opened).toHaveBeenLastCalledWith(legal["CC-BY-NC-SA-4.0"]);
  // Choosing another license links that license's own text.
  await user.selectOptions(within(dialog).getByRole("combobox", { name: "License" }), "CC-BY-SA-4.0");
  expect(within(dialog).queryByRole("button", { name: "CC BY-NC-SA 4.0" })).toBeNull();
  await user.click(within(dialog).getByRole("button", { name: "CC BY-SA 4.0" }));
  expect(opened).toHaveBeenLastCalledWith(legal["CC-BY-SA-4.0"]);
});

it("fixes the license that shared items allow, and counts items whose license doesn't allow sharing", async () => {
  const { dialog } = await fixture(bridge => {
    bridge.sharing.licensedItems = [{ key: "m-dividend-growth", name: "Dividend growth check", license: "CC-BY-SA-4.0" }];
    bridge.sharing.summaryOverrides = { counts: { perspective: 1, methods: 14, principles: 6, names: 312, links: 1204, sources: 88, leftOut: 41, leftOutLicense: 2 } };
  });
  const select = within(dialog).getByRole("combobox", { name: "License" });
  await waitFor(() => expect(select).toBeDisabled());
  expect(select).toHaveValue("CC-BY-SA-4.0");
  expect(select).toHaveAccessibleDescription(/must keep the same license\. Set by the license of items that came from shared Loupists\./);
  expect(within(dialog).getByText("Left out: 2 items whose license doesn't allow sharing")).toBeVisible();
  expect(within(dialog).getByRole("button", { name: "Save as folder" })).toBeEnabled();
});

it("names the items whose licenses clash and keeps outputs off until one side is left out", async () => {
  const { bridge, dialog, user } = await fixture(bridge => {
    bridge.sharing.licensedItems = [
      { key: "m-dividend-growth", name: "Dividend growth check", license: "CC-BY-SA-4.0" },
      { key: "p-one-quarter", name: "Never size a position on one quarter.", license: "CC-BY-NC-4.0" },
    ];
  });
  const exported = vi.spyOn(bridge, "shareExport");
  const conflict = within(dialog).getByRole("region", { name: "License" });
  expect(conflict).toHaveTextContent("Some items came from shared Loupists under licenses that can't be shared together.");
  const rows = within(conflict).getAllByRole("listitem");
  expect(rows.map(row => row.textContent)).toEqual([
    "Dividend growth checkCC BY-SA 4.0Leave it out",
    "Never size a position on one quarter.CC BY-NC 4.0Leave it out",
  ]);
  expect(within(dialog).queryByRole("combobox", { name: "License" })).toBeNull();
  for (const name of ["Publish to GitHub", "Save as folder", "Save as ZIP"]) {
    expect(within(dialog).getByRole("button", { name })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name })).toHaveAccessibleDescription("Leave out some of the items whose licenses clash first.");
  }

  // Leaving out the share-alike item leaves the non-commercial licenses, which Rust offers again.
  await user.click(within(rows[0]).getByRole("button", { name: "Leave it out" }));
  const select = await within(dialog).findByRole("combobox", { name: "License" });
  expect(within(select).getAllByRole("option").map(option => option.textContent)).toEqual(["CC BY-NC-SA 4.0", "CC BY-NC 4.0"]);
  expect(select).toHaveValue("CC-BY-NC-SA-4.0");
  await user.click(within(dialog).getByRole("button", { name: "Save as folder" }));
  await waitFor(() => expect(exported).toHaveBeenCalledWith(expect.objectContaining({ license: "CC-BY-NC-SA-4.0", excludedItemKeys: ["m-dividend-growth"] })));
});

it("keeps Gallery listing unavailable when this build names no listing terms", async () => {
  const { bridge, dialog, user } = await fixture(bridge => {
    bridge.sharing.account = { clientConfigured: true, indexConfigured: true, galleryTermsUrl: null, login: "kimj" };
  });
  const listing = vi.spyOn(bridge, "galleryListingRequest");
  const status = vi.spyOn(bridge, "galleryListingStatus");
  await user.click(within(dialog).getByRole("button", { name: "Publish to GitHub" }));
  await user.click(await screen.findByRole("button", { name: "Publish to GitHub" }));
  expect(await screen.findByText("Published to github.com/kimj/loupist-quality-compounder")).toBeVisible();
  await waitFor(() => expect(status).toHaveBeenCalled());
  await act(async () => { await status.mock.results[0]!.value; });
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(screen.queryByRole("button", { name: "List it in the Gallery" })).toBeNull();
  expect(listing).not.toHaveBeenCalled();
});

it("lists in the Gallery only after the user agrees to the listing terms, accepting that exact address", async () => {
  const termsUrl = "https://example.org/loupist/gallery-terms";
  const { bridge, dialog, user } = await fixture(bridge => {
    bridge.sharing.account = { clientConfigured: true, indexConfigured: true, galleryTermsUrl: termsUrl, login: "kimj" };
  });
  const listing = vi.spyOn(bridge, "galleryListingRequest");
  const opened = vi.spyOn(bridge, "openExternalUrl").mockResolvedValue(undefined);
  await user.click(within(dialog).getByRole("button", { name: "Publish to GitHub" }));
  await user.click(await screen.findByRole("button", { name: "Publish to GitHub" }));
  const agree = await screen.findByRole("checkbox", { name: "I agree to the Gallery listing terms" });
  const list = screen.getByRole("button", { name: "List it in the Gallery" });
  expect(agree).not.toBeChecked();
  expect(list).toBeDisabled();
  expect(list).toHaveAccessibleDescription(/nobody reviews it by hand/);

  // The terms phrase opens the terms; reading them does not tick the box.
  await user.click(screen.getByRole("button", { name: "Gallery listing terms" }));
  expect(opened).toHaveBeenCalledWith(termsUrl);
  expect(agree).not.toBeChecked();

  await user.click(agree);
  await user.click(list);
  expect(await screen.findByText("Listing requested")).toBeVisible();
  expect(listing).toHaveBeenCalledWith({ loupistId: "loupist-quality", acceptedTerms: true, termsUrl });
  expect(screen.queryByRole("checkbox")).toBeNull();
});

it("shows Listed in the Gallery when the index lists this commit", async () => {
  const { dialog, user } = await fixture(bridge => {
    bridge.sharing.account = { clientConfigured: true, indexConfigured: true, galleryTermsUrl: null, login: "kimj" };
    bridge.sharing.listingStatus = "listed";
  });
  await user.click(within(dialog).getByRole("button", { name: "Publish to GitHub" }));
  await user.click(await screen.findByRole("button", { name: "Publish to GitHub" }));
  expect(await screen.findByText("Listed in the Gallery")).toBeVisible();
  expect(screen.queryByRole("checkbox", { name: "List it in the Gallery" })).toBeNull();
});


it("reports clipboard failure and announces Copied only after a successful retry", async () => {
  const { dialog, user } = await fixture(bridge => { bridge.sharing.account = { ...bridge.sharing.account, login: "kimj" }; });
  const copy = vi.spyOn(navigator.clipboard, "writeText").mockRejectedValueOnce(new Error("clipboard denied"));
  await user.click(within(dialog).getByRole("button", { name: "Publish to GitHub" }));
  await user.click(await screen.findByRole("button", { name: "Publish to GitHub" }));
  await screen.findByText("Published to github.com/kimj/loupist-quality-compounder");
  await user.click(screen.getByRole("button", { name: "Copy link" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't copy the link. Try again.");
  expect(screen.queryByText("Link copied")).toBeNull();
  copy.mockResolvedValue(undefined);
  await user.click(screen.getByRole("button", { name: "Copy link" }));
  expect(await screen.findByText("Link copied")).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();
});
