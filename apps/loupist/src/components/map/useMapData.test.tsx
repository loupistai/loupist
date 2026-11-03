import { act, renderHook, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { useMapData } from "./useMapData";
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
