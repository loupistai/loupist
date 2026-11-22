import { act, renderHook, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { PreviewBridge } from "../../preview/fixtures";
import { useMapData, useMapDetail } from "./useMapData";
it('ignores late maps when the Loupist changes', async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const map = await bridge.knowledgeMap({ loupistId: 'loupist-quality' });
  let resolve!: (value: typeof map) => void;
  vi.spyOn(bridge, 'knowledgeMap').mockImplementationOnce(() => new Promise((done) => { resolve = done; }));
  const { result, rerender } = renderHook(({ loupistId }) => useMapData(bridge, loupistId, 0), { initialProps: { loupistId: 'loupist-quality' } });
  rerender({ loupistId: 'loupist-empty' });
  await waitFor(() => expect(result.current.map?.nodes).toEqual([]));
  await act(async () => resolve(map));
  expect(result.current.map?.nodes).toEqual([]);
});

const location = { surface: "world" as const, loupistId: "loupist-quality", recordId: "preview-world", revisionId: "first" };

it("advances the overview before showing a detail from a newer revision", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  const read = bridge.knowledgeWorldQuery.bind(bridge);
  const query = vi.spyOn(bridge, "knowledgeWorldQuery").mockImplementation(async request => ({ ...await read(request), revisionId: "second" }));
  const advance = vi.fn();
  const { result, rerender } = renderHook(({ revisionId }) => useMapDetail(bridge, { ...location, revisionId }, "preview-node-0", "node", advance), { initialProps: { revisionId: "first" } });

  await waitFor(() => expect(advance).toHaveBeenCalledOnce());
  expect(result.current.graph).toBeUndefined();
  expect(result.current.error).toBe(false);
  rerender({ revisionId: "second" });
  expect(result.current.graph?.revisionId).toBe("second");
  expect(query).toHaveBeenCalledWith(expect.objectContaining({ revisionId: "second", centerNodeId: "preview-node-0" }));
});

it("retries busy reads twice and asks the host again after a terminal failure", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  const read = bridge.knowledgeWorldQuery.bind(bridge);
  const query = vi.spyOn(bridge, "knowledgeWorldQuery").mockRejectedValue({ code: "world_busy" });
  const { result } = renderHook(() => useMapDetail(bridge, location, "claim-id", "claim"));

  expect(result.current.error).toBe(false);
  await waitFor(() => expect(result.current.error).toBe(true));
  expect(query).toHaveBeenCalledTimes(3);
  query.mockImplementation(async request => ({ ...await read(request), revisionId: "latest" }));
  act(() => result.current.retry());
  await waitFor(() => expect(result.current.graph?.revisionId).toBe("latest"));
  expect(result.current.error).toBe(false);
  expect(query).toHaveBeenLastCalledWith(expect.objectContaining({ centerClaimId: "claim-id" }));
});

it("surfaces a real read failure after one attempt", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  const query = vi.spyOn(bridge, "knowledgeWorldQuery").mockRejectedValue({ code: "unauthorized" });
  const { result } = renderHook(() => useMapDetail(bridge, location, "preview-node-0"));
  await waitFor(() => expect(result.current.error).toBe(true));
  expect(query).toHaveBeenCalledOnce();
});
