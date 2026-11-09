import { enableSharingPreviewHarness, SHARING_UI_IN_RELEASE, sharingReleaseVisible } from "./releaseVisibility";

it("keeps pack sharing out of the user app until the constant is turned on", () => {
  expect(SHARING_UI_IN_RELEASE).toBe(false);
  expect(sharingReleaseVisible()).toBe(false);
});

it("lets the preview harness open the same screens", () => {
  enableSharingPreviewHarness();
  expect(sharingReleaseVisible()).toBe(true);
});
