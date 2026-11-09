/**
 * World and Lens pack sharing stays implemented. This release does not show
 * it in the user app. The development preview harness opts in so its
 * scenarios can still open the screens. Set this to true to restore the
 * shipped entry points. There is no settings switch and no address that
 * turns the screens on.
 */
export const SHARING_UI_IN_RELEASE = false;

let previewHarness = false;

/** Preview harness only (`preview/main.tsx` and the scenario suite). The shipped app never calls this. */
export function enableSharingPreviewHarness(): void {
  previewHarness = true;
}

export function sharingReleaseVisible(): boolean {
  return SHARING_UI_IN_RELEASE || previewHarness;
}
