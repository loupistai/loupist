import { act, renderHook } from "@testing-library/react";
import type { PackOpenResult } from "../contracts/sharing";
import { APP_DESTINATIONS, destinationForKey } from "./navigation";
import { surfaceReducer, surfaceView, useAppSurface, type AppSurface } from "./useAppSurface";

const base: AppSurface = { tab: "chat", loupistSettings: null, assetPage: null, gallery: false, packReview: null };
const review = { status: "ok" } as unknown as PackOpenResult;

it("closes only Loupist settings on a page change, and returns to the layer underneath", () => {
  let surface = surfaceReducer(base, { type: "openAssetPage", page: { loupistId: "a", assetId: "x", key: 1 } });
  surface = surfaceReducer(surface, { type: "openLoupistSettings", loupistId: "a", remove: false });
  expect(surfaceView(surface, "a")).toMatchObject({ loupistSettingsOpen: true, assetPageOpen: true, covered: true, chatVisible: false, chatActive: false });
  surface = surfaceReducer(surface, { type: "closeLoupistSettings" });
  expect(surfaceView(surface, "a")).toMatchObject({ assetPageOpen: true, chatVisible: false, chatActive: true });
  surface = surfaceReducer(surfaceReducer(surface, { type: "openLoupistSettings", loupistId: "a", remove: false }), { type: "page", tab: "schedule" });
  expect(surface).toMatchObject({ tab: "schedule", loupistSettings: null, assetPage: { assetId: "x" } });
});

it("keeps the Gallery under a pack review and hides another Loupist's asset page", () => {
  let surface = surfaceReducer(surfaceReducer(base, { type: "openGallery" }), { type: "openPackReview", result: review, use: true });
  expect(surface.packReview).toEqual({ result: review, use: true });
  surface = surfaceReducer(surface, { type: "closePackReview" });
  expect(surface.gallery).toBe(true);
  expect(surfaceReducer(surface, { type: "closeLayers" })).toMatchObject({ gallery: false, packReview: null, assetPage: null });
  const asset = surfaceReducer(base, { type: "openAssetPage", page: { loupistId: "a", key: 2 } });
  expect(surfaceView(asset, "b").assetPageOpen).toBe(false);
  expect(surfaceView({ ...asset, tab: "settings" }, "a").assetPageOpen).toBe(false);
  expect(surfaceReducer(asset, { type: "keepAssetPageFor", loupistId: "b" }).assetPage).toBeNull();
  expect(surfaceReducer(asset, { type: "keepAssetPageFor", loupistId: "a" })).toBe(asset);
});

it("names the open asset page as the page shows it, and ignores a page already replaced", () => {
  const opened = surfaceReducer(base, { type: "openAssetPage", page: { loupistId: "a", assetId: "x", name: "Hanul Precision", key: 3 } });
  expect(surfaceReducer(opened, { type: "nameAssetPage", key: 3, name: "한울정밀" }).assetPage).toMatchObject({ assetId: "x", name: "한울정밀", key: 3 });
  expect(surfaceReducer(opened, { type: "nameAssetPage", key: 2, name: "한울정밀" })).toBe(opened);
  expect(surfaceReducer(opened, { type: "nameAssetPage", key: 3, name: "Hanul Precision" })).toBe(opened);
});

it("opens Map and Lens as full pages and reads the destination keys from one list", () => {
  localStorage.setItem("loupist.lastView", "schedule");
  const { result } = renderHook(() => useAppSurface());
  expect(result.current.surface.tab).toBe("schedule");
  act(() => result.current.goTo("map"));
  expect(result.current.surface.tab).toBe("map");
  expect(localStorage.getItem("loupist.lastView")).toBe("map");
  act(() => result.current.goTo("lens"));
  expect(result.current.surface.tab).toBe("lens");
  const epoch = result.current.navigationEpoch.current;
  act(() => result.current.goTo("schedule"));
  expect(result.current.surface.tab).toBe("schedule");
  expect(result.current.navigationEpoch.current).toBe(epoch + 1);
  expect(APP_DESTINATIONS.map((item) => item.id)).toEqual(["chat", "map", "lens", "schedule"]);
  expect(destinationForKey("2")?.page).toBe("map");
  expect(destinationForKey("3")?.page).toBe("lens");
  expect(destinationForKey("5")).toBeUndefined();
  expect(destinationForKey("0")).toBeUndefined();
});
