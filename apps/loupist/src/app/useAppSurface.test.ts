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

it("opens World and Lens as panels beside Chat and reads the destination keys from one list", () => {
  localStorage.setItem("loupist.lastView", "schedule");
  const selectKind = vi.fn();
  const { result } = renderHook(() => useAppSurface({ hydrated: true, selectKind }));
  expect(result.current.surface.tab).toBe("schedule");
  act(() => result.current.goTo("map"));
  expect(selectKind).toHaveBeenLastCalledWith("world");
  expect(result.current.surface.tab).toBe("chat");
  expect(localStorage.getItem("loupist.lastView")).toBe("chat");
  const epoch = result.current.navigationEpoch.current;
  act(() => result.current.goTo("schedule"));
  expect(result.current.surface.tab).toBe("schedule");
  expect(result.current.navigationEpoch.current).toBe(epoch + 1);
  expect(APP_DESTINATIONS.map((item) => item.id)).toEqual(["chat", "map", "lens", "schedule"]);
  expect(destinationForKey("3")?.id).toBe("lens");
  expect(destinationForKey("5")).toBeUndefined();
  expect(destinationForKey("0")).toBeUndefined();
});

it("moves a saved Map or Lens page into its panel once the panel state is read", () => {
  localStorage.setItem("loupist.lastView", "lens");
  const selectKind = vi.fn();
  const { rerender } = renderHook(({ hydrated }) => useAppSurface({ hydrated, selectKind }), { initialProps: { hydrated: false } });
  expect(selectKind).not.toHaveBeenCalled();
  rerender({ hydrated: true });
  expect(selectKind).toHaveBeenCalledExactlyOnceWith("lens");
  expect(localStorage.getItem("loupist.lastView")).toBe("chat");
});
