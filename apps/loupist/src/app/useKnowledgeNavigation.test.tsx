import { act, renderHook } from "@testing-library/react";
import { useKnowledgeNavigation } from "./useKnowledgeNavigation";

it("keeps each Loupist's selected view, query and Reader return state independently", () => {
  const { result, rerender } = renderHook(({ loupistId }) => useKnowledgeNavigation(loupistId, true), { initialProps: { loupistId: "loupist-a" } });
  const graph = { loupistId: "loupist-a", surface: "world" as const, recordId: "world-a", revisionId: "snapshot-a", selection: { kind: "node" as const, id: "entity-a" }, view: { search: "capacity", camera: { x: 70, y: 20, scale: 1.8 } } };
  act(() => result.current.patch(graph));
  act(() => result.current.open({ loupistId: "loupist-a", surface: "source", recordId: "source-a", revisionId: "capture-a", selection: { kind: "source", sourceId: "capture" }, view: { readerScrollTop: 220 } }));
  rerender({ loupistId: "loupist-b" });
  expect(result.current.navigation?.current).toEqual({ loupistId: "loupist-b", surface: "home" });
  act(() => result.current.patch({ loupistId: "loupist-b", surface: "lens", view: { search: "unit economics" } }));
  rerender({ loupistId: "loupist-a" });
  expect(result.current.navigation?.current.view?.readerScrollTop).toBe(220);
  act(() => result.current.back());
  expect(result.current.navigation?.current).toEqual(graph);
  rerender({ loupistId: "loupist-b" });
  expect(result.current.navigation?.current.view?.search).toBe("unit economics");
});

it("stores Graph, Sources and Lens views without using Reader history as tab storage", () => {
  const { result } = renderHook(() => useKnowledgeNavigation("loupist", true));
  const source = { loupistId: "loupist", surface: "source" as const, view: { search: "earnings", sourceFilter: "attention" as const, sourceScrollTop: 150, focusSourceId: "report" } };
  const lens = { loupistId: "loupist", surface: "lens" as const, recordId: "method", view: { search: "margins", readerScrollTop: 80 } };
  act(() => result.current.patch(source));
  act(() => result.current.patch(lens));
  expect(result.current.navigation?.past).toEqual([]);
  expect(result.current.navigation?.views.source).toEqual(source);
  expect(result.current.navigation?.views.lens).toEqual(lens);
  expect(result.current.navigation?.worldSurface).toBe("source");
  act(() => result.current.open({ loupistId: "loupist", surface: "source", recordId: "evidence", revisionId: "source-version", selection: { kind: "source", sourceId: "evidence" } }));
  act(() => result.current.back());
  expect(result.current.navigation?.current).toEqual(lens);
});
