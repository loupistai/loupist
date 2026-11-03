import { render } from "@testing-library/react";
import type { ForceGraphProps } from "react-force-graph-2d";
import MapCanvas, { type MapCanvasLink } from "../map/MapCanvas";
import type { PositionedNode } from "../map/layout";
const renderer = vi.hoisted(() => ({ props: null as ForceGraphProps<PositionedNode, MapCanvasLink> | null }));
vi.mock('react-force-graph-2d', async () => {
 const { forwardRef, useImperativeHandle } = await import('react');
 return { default: forwardRef((props: ForceGraphProps<PositionedNode, MapCanvasLink>, ref) => {
   renderer.props = props;
   useImperativeHandle(ref, () => ({ centerAt: () => ({ x: 0, y: 0 }), zoom: () => 1, zoomToFit: vi.fn(), graph2ScreenCoords: (x: number, y: number) => ({ x: x + 100, y: y + 100 }) }));
   return <canvas />;
 }) };
});
const nodes: PositionedNode[] = Array.from({ length: 12 }, (_, i) => ({ id: `n${i}`, label: `Name ${i}`, degree: 12 - i, kind: 'entity', entityKind: 'Organization', watched: false, held: false, x: i * 40, y: 0, fx: i * 40, fy: 0 }));
const props = { nodes, edges: [{ id: 'edge', kind: 'claim' as const, source: 'n0', target: 'n1' }], scopeKey: 'compact-test', controls: { current: null }, stats: '12 names', onSelect: vi.fn(), onClear: vi.fn() };
beforeEach(() => { vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(900); vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(700); });
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); });
it('labels only the top ten nodes even when zoomed in', () => {
 render(<MapCanvas {...props} compact={{}} />);
 const context = { save: vi.fn(), restore: vi.fn(), measureText: () => ({ width: 12 }), strokeText: vi.fn(), fillText: vi.fn() };
 renderer.props!.onRenderFramePost?.(context as unknown as CanvasRenderingContext2D, 2);
 expect(context.fillText).toHaveBeenCalledTimes(10);
 expect(context.fillText.mock.calls.map(call => call[0])).toEqual(nodes.slice(0, 10).map(node => node.label));
});
it('suppresses glow and scaling for reduced motion without losing selection', () => {
 vi.spyOn(window, 'matchMedia').mockImplementation(query => ({ matches: query.includes('prefers-reduced-motion'), media: query, onchange: null, addListener: vi.fn(), removeListener: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn(), dispatchEvent: vi.fn() }));
 render(<MapCanvas {...props} compact={{ highlightIds: new Set(['n0']), glowNodeIds: new Set(['n0']) }} />);
 const context = { save: vi.fn(), restore: vi.fn(), beginPath: vi.fn(), arc: vi.fn(), translate: vi.fn(), scale: vi.fn(), stroke: vi.fn(), setLineDash: vi.fn(), fill: vi.fn() };
 renderer.props!.nodeCanvasObject?.(nodes[0], context as unknown as CanvasRenderingContext2D, 1);
 expect(context.scale).not.toHaveBeenCalled();
 expect(context.fill).toHaveBeenCalledTimes(1);
 expect(context.arc).toHaveBeenCalledTimes(2);
});
