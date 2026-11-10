/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, PreviewBridge } from "../fixtures";
import { MOCK_PACK_PREVIEW } from "../../bridge/mock/sharing";
import { base, click, idle, navigate, openNavigation, select, selectPanel, until, type PreviewScenario } from "./kit";

/** W19 sharing: Share sheet, link or file, review before adding, suggestions, origin and deep links. */
export function sharingScenarios(): PreviewScenario[] {
  const openMemoryPage = async (root: HTMLElement) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    await selectPanel("lens");
    await until(() => select(".memory-page-tabs"), "What the Loupist knows did not load");
  };
  const openShare = async (root: HTMLElement) => { await openMemoryPage(root); await click("[data-share-loupist]"); };
  const publishFromSheet = async () => {
    await click('[data-share-sheet="loupist"] [data-share-publish]');
    await click('.share-sheet[data-step="repo"] .share-primary');
  };
  const openLinkDialog = async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    await openNavigation(".loupist-header-new");
    await click(".loupist-header-new");
    await click("[data-loupist-shared-entry] [data-use-link-or-file]");
  };
  /** W19.6: "+ New Loupist" › Browse the Gallery, in a build with a community index. */
  const withIndex = (bridge: PreviewBridge) => { bridge.sharing.account = { ...bridge.sharing.account, indexConfigured: true }; };
  const openGallery = async () => {
    await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
    await openNavigation(".loupist-header-new");
    await click(".loupist-header-new");
    await click("[data-loupist-shared-entry] [data-browse-gallery]");
    await until(() => select("[data-gallery]"), "The Gallery did not open");
  };
  /** W19.5: the shared Loupists section on "What {Loupist} knows". */
  const openSharedSection = async (root: HTMLElement) => {
    await openMemoryPage(root);
    await click('.memory-page-tabs [role="tab"]:nth-child(1)');
    const section = await until(() => select("[data-shared-packs]"), "Shared Loupists you added did not show");
    // The update check runs as the section opens; scroll once its result is in place.
    await until(() => select("[data-pack-update], [data-pack-removed-banner]"), "No update or banner showed");
    section.scrollIntoView?.({ block: "start" });
  };
  const openReview = async () => {
    await openLinkDialog();
    await click("[data-pack-link-dialog] .share-outputs button");
    await until(() => select("[data-pack-review]"), "Review before adding did not open");
  };
  const sharing = (title: string, id: string, landmark: () => Element | null, enter: (root: HTMLElement) => Promise<void>, configure: (bridge: PreviewBridge) => void | Promise<void> = () => {}): PreviewScenario => ({
    ...base(id, title, landmark, enter),
    setup: async bridge => { await prepareFixture(bridge); await configure(bridge); },
  });
  const signedIn = (bridge: PreviewBridge) => { bridge.sharing.account = { ...bridge.sharing.account, login: "kimj" }; };
  /** The License field sits under the sections; bring it into the sheet's view. */
  const openShareAt = (target: string) => async (root: HTMLElement) => {
    await openShare(root);
    (await until(() => select(target), `${target} did not show`)).scrollIntoView?.({ block: "start" });
  };
  return [
    sharing("Share sheet", "share-sheet", () => select('[data-share-sheet="loupist"] .share-count'), openShare),
    sharing("Share sheet with private details to check", "share-sheet-findings", () => select("[data-share-findings]"), openShare, bridge => bridge.sharing.setFindings()),
    sharing("Share sheet with nothing to share", "share-sheet-nothing", () => select("[data-share-nothing]"), openShare,
      bridge => { bridge.sharing.summaryOverrides = { counts: { perspective: 0, methods: 0, principles: 0, names: 0, links: 0, sources: 0, leftOut: 0, leftOutLicense: 0 } }; }),
    sharing("Sign in to GitHub with a device code", "share-sheet-github-code", () => select("[data-device-code]"),
      async root => { await openShare(root); await click('[data-share-sheet="loupist"] [data-share-publish]'); }, bridge => { bridge.sharing.signInMode = "wait"; }),
    sharing("Publishing to GitHub", "share-sheet-publishing", () => select("[data-share-progress]")?.textContent?.match(/\d/) ? select("[data-share-progress]") : null,
      async root => { await openShare(root); await publishFromSheet(); }, bridge => { signedIn(bridge); bridge.sharing.holdPublish = true; }),
    sharing("Published to GitHub", "share-sheet-published", () => select("[data-share-published]"), async root => { await openShare(root); await publishFromSheet(); }, signedIn),
    sharing("Update the recorded repository", "share-sheet-update-repo", () => select('.share-sheet[data-step="repo"] .share-primary'),
      async root => { await openShare(root); await click('[data-share-sheet="loupist"] [data-share-publish]'); },
      bridge => { signedIn(bridge); bridge.sharing.summaryOverrides = { repo: { fullName: "kimj/loupist-quality-compounder", url: "https://github.com/kimj/loupist-quality-compounder", version: 4 }, nextVersion: 5 }; }),
    // Licensing: Rust's NC-SA default with items whose license keeps them out, and two shared items whose licenses clash.
    sharing("Share sheet license with the NC-SA default", "share-sheet-license", () => select("[data-share-license-summary]"), openShareAt("[data-share-left-out-license]"),
      bridge => { bridge.sharing.summaryOverrides = { counts: { perspective: 1, methods: 14, principles: 6, names: 312, links: 1204, sources: 88, leftOut: 41, leftOutLicense: 2 } }; }),
    sharing("Share sheet with licenses that can't be shared together", "share-sheet-license-conflict", () => select("[data-share-license-conflict]"), openShareAt("[data-share-license-conflict]"),
      bridge => {
        bridge.sharing.licensedItems = [
          { key: "m-5a1c9e2b7d3f4a60", name: "Dividend growth check", license: "CC-BY-SA-4.0" },
          { key: "p-6b2d0f3c8e4a5b71", name: "Never size a position on one quarter.", license: "CC-BY-NC-4.0" },
        ];
      }),
    sharing("Agree to the Gallery listing terms", "share-sheet-listing-terms", () => select("[data-gallery-terms]"), async root => { await openShare(root); await publishFromSheet(); },
      bridge => { signedIn(bridge); bridge.sharing.account = { ...bridge.sharing.account, indexConfigured: true, galleryTermsUrl: "https://example.org/loupist/gallery-terms" }; }),
    sharing("No GitHub client in this build", "share-sheet-no-github", () => select("[data-github-missing]"), openShare,
      bridge => { bridge.sharing.account = { clientConfigured: false, indexConfigured: false, galleryTermsUrl: null, login: null }; }),
    sharing("Share this method", "share-method", () => select('[data-share-sheet="item"] .share-item-row .share-section-note'), async root => {
      await openMemoryPage(root);
      const row = await until(() => [...document.querySelectorAll<HTMLButtonElement>(".memory-page-list .memory-row")].find(button => button.querySelector(".memory-marker.is-method")), "No method row");
      row.click();
      await click(".memory-reader [data-share-item]");
    }),
    sharing("Start from a shared Loupist", "new-loupist-shared-entry", () => select("[data-loupist-shared-entry]"), async () => {
      await until(() => select("textarea[data-chat-composer]"), "Chat did not initialize");
      await openNavigation(".loupist-header-new");
      await click(".loupist-header-new");
    }),
    sharing("Open a shared Loupist from a link or file", "pack-link-dialog", () => select("[data-pack-link-dialog] input"), openLinkDialog),
    sharing("Review before adding", "pack-review", () => select("[data-pack-review] .pack-review-sections"), openReview),
    sharing("Review a method with a warning", "pack-review-warnings", () => select(".pack-review-item-body [data-flagged]"), async () => {
      await openReview();
      await click('.pack-review-items li[data-flagged] .pack-review-item');
    }),
    sharing("Add to an existing Loupist", "pack-review-merge", () => select('.pack-review-section[data-disabled]'), async () => {
      await openReview();
      await click('.pack-review-merge [role="radio"]');
    }),
    sharing("A pack that can't be added", "pack-review-invalid", () => select("[data-pack-rejected]"), openReview, bridge => bridge.sharing.rejectWith("unsafe")),
    sharing("Review a Personal-licensed pack", "pack-review-personal", () => select("[data-pack-not-redistributable]"), openReview, bridge => {
      const manifest = { ...MOCK_PACK_PREVIEW.manifest, license: "LicenseRef-Loupist-Personal" as const, licenseUrl: "https://example.org/loupist/personal-terms", redistributable: false };
      bridge.sharing.openResult = { kind: "preview", preview: { ...MOCK_PACK_PREVIEW, manifest } };
    }),
    sharing("A pack removed from the Gallery", "pack-review-blocked", () => select("[data-pack-blocked]"), openReview,
      bridge => { bridge.sharing.openResult = { kind: "preview", preview: { ...MOCK_PACK_PREVIEW, blocked: "reported" } }; }),
    sharing("Suggestions from a shared Loupist", "pack-suggestions", () => select("[data-pack-suggestions] .pack-suggestion"), openMemoryPage,
      bridge => { bridge.sharing.seedImport("loupist-quality"); }),
    sharing("A shared name in the Map inspector", "map-inspector-shared", () => select(".map-inspector [data-shared-origin]"), async root => {
      await navigate(root, "map");
      const map = await until(() => select(".map-view"), "Map did not load");
      const input = await until(() => map.querySelector<HTMLInputElement>('[role="combobox"]'), "Map search did not load");
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Apple");
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await click('[role="option"]', map);
      await until(() => select(".map-inspector"), "Inspector did not open");
    }, bridge => { bridge.packOrigin = async () => ({ importId: "imp-1", packName: "Semiconductor supply chain", author: "Kim J." }); }),
    // A link held from a cold start: Rust asks once onboarding is done, and the dialog opens over Chat.
    sharing("Open a shared Loupist from a link", "deep-link-confirm", () => select("[data-deep-link-dialog]"), idle, bridge => { bridge.sharing.receiveDeepLink({ status: "ok", source: "github.com/kimj/loupist-semiconductor-supply-chain" }, true); }),
    // W19.6: the community Gallery under "+ New Loupist".
    sharing("Gallery of shared Loupists", "gallery", () => select("[data-gallery-card] .gallery-card-image img"), openGallery, withIndex),
    sharing("Gallery while offline", "gallery-offline", () => select("[data-gallery-offline]"), openGallery, bridge => {
      withIndex(bridge);
      bridge.sharing.gallery = { ...bridge.sharing.gallery, status: "offline", fetchedAtMs: Date.parse("2026-10-26T18:00:00Z") };
    }),
    sharing("Gallery with no match", "gallery-no-match", () => select("[data-gallery-empty]"), async () => {
      await openGallery();
      const input = await until(() => select("[data-gallery] input[type=search]") as HTMLInputElement | null, "Gallery search did not load");
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "tanker freight");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    }, withIndex),
    // W19.5: a newer version of a shared Loupist, checked when the section opens.
    sharing("Update from a shared Loupist", "pack-update-diff", () => select("[data-pack-update]"), openSharedSection,
      bridge => { bridge.sharing.seedImport("loupist-quality", { version: 3 }); }),
    sharing("A shared Loupist removed from the Gallery", "pack-removed-banner", () => select("[data-pack-removed-banner]"), openSharedSection,
      bridge => { withIndex(bridge); bridge.sharing.updateCheck = { kind: "upToDate" }; bridge.sharing.seedImport("loupist-quality", { blockedReason: "reported" }); }),
  ];
}
