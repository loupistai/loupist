import { useState } from "react";
import { cleanup, render, screen, within, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { KnowledgeLocation } from "../../types";
import { setPreviewLanguage } from "../../i18n/useLanguage";
import { MapView } from "./MapView";
const canvasFailure = vi.hoisted(() => ({ active: false }));
vi.mock('./MapCanvas', () => ({ default: ({ nodes = [], edges = [], overlayIds }: { nodes?: { kind: string; entityKind?: string }[]; edges?: { id: string }[]; overlayIds?: Set<string> }) => { if (canvasFailure.active) throw new Error("Canvas unavailable"); return <div data-testid="map-canvas" data-kinds={nodes.map(node => node.kind).join(",")} data-nodes={nodes.map(node => `${node.kind}|${node.entityKind ?? ""}`).join(",")} data-edges={edges.map(edge => edge.id).join(",")} data-overlay={[...(overlayIds ?? [])].sort().join(",")} />; } }));
const hintCopy = "Click a name to see what your Loupist knows about it and where that came from.";
function mockMapBox(width: number, height: number) {
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(width);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(height);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const stage = this.classList.contains("map-stage");
    const boxWidth = stage ? width : 160;
    const boxHeight = stage ? height : 32;
    return { x: 0, y: 0, top: 0, left: 0, width: boxWidth, height: boxHeight, right: boxWidth, bottom: boxHeight, toJSON() { return {}; } } as DOMRect;
  });
}
async function setup() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const catalog = await bridge.knowledgeCatalog({ loupistId: 'loupist-quality' });
  const onAsk = vi.fn(), onOpen = vi.fn();
  function Harness() {
    const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: catalog.loupistId, surface: 'world', recordId: catalog.worlds[0]?.recordId, revisionId: catalog.worlds[0]?.revisionId });
    return <MapView bridge={bridge} catalog={catalog} location={location} onPatch={setLocation} onOpen={onOpen} onAsk={onAsk} />;
  }
  render(<Harness />);
  await screen.findByText('37 on the map · 36 relationships');
  return { bridge, onAsk, onOpen, user: userEvent.setup() };
}
it('shows the first-map guidance when the host has only seeded its standing Lens', async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const catalog = await bridge.knowledgeCatalog({ loupistId: 'loupist-quality' });
  const map = await bridge.knowledgeMap({ loupistId: catalog.loupistId });
  map.nodes = map.nodes.filter(node => node.kind === 'lens_perspective');
  expect(map.nodes.length).toBeGreaterThan(0);
  map.edges = [];
  vi.spyOn(bridge, 'knowledgeMap').mockResolvedValue(map);
  const addSource = vi.fn(), onOpen = vi.fn();
  render(<MapView bridge={bridge} catalog={catalog} location={{ loupistId: catalog.loupistId, surface: 'world' }} onPatch={vi.fn()} onOpen={onOpen} onAsk={vi.fn()} onAddSource={addSource} />);
  expect(await screen.findByRole('heading', { name: "Your Loupist's map grows as you research" })).toBeVisible();
  expect(screen.queryByText('No names match these filters.')).not.toBeInTheDocument();
  expect(screen.queryByText('0 on the map · 0 relationships')).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Library' })).toBeVisible();
  screen.getByRole('button', { name: 'Add source' }).click();
  expect(addSource).toHaveBeenCalledOnce();
  expect(screen.getByRole('button', { name: 'Ask in Chat' })).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Lens wiki' })).toBeNull();
});
it('searches and selects with the keyboard, opens the inspector, and attaches the exact entity', async () => {
  const { user, onAsk } = await setup();
  await user.type(screen.getByRole('combobox'), 'Apple');
  await screen.findByRole('option', { name: /Apple/ });
  await user.keyboard('{Enter}');
  expect(await screen.findByRole('heading', { name: 'Apple' })).toHaveFocus();
  await user.click(screen.getByRole('button', { name: 'Ask about this in Chat' }));
  expect(onAsk).toHaveBeenCalledWith(expect.objectContaining({ kind: 'world_node', objectId: 'map-entity-00', loupistId: 'loupist-quality' }));
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('complementary')).not.toBeInTheDocument();
  expect(screen.getByRole('combobox')).toHaveFocus();
});
it('provides list navigation and combines watchlist and kind filters', async () => {
  const { user } = await setup();
  await user.click(screen.getByRole('button', { name: 'Filter' }));
  await user.click(screen.getByLabelText('Only what I follow and hold'));
  await screen.findByText('3 on the map · 2 relationships');
  await user.keyboard('{Escape}');
  await user.click(screen.getByRole('button', { name: 'More actions' }));
  await user.click(screen.getByRole('menuitemcheckbox', { name: 'List view' }));
  const table = screen.getByRole('table', { name: 'List view' });
  expect(within(table).getAllByRole('row')).toHaveLength(4);
  await user.click(within(table).getByRole('button', { name: 'TSMC' }));
  expect(await screen.findByRole('heading', { name: 'TSMC' })).toHaveFocus();
});
it('retries a failed projection without switching Loupist', async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const catalog = await bridge.knowledgeCatalog({ loupistId: 'loupist-quality' });
  const request = vi.spyOn(bridge, 'knowledgeMap').mockRejectedValueOnce(new Error('offline'));
  render(<MapView bridge={bridge} catalog={catalog} location={{ loupistId: catalog.loupistId, surface: 'world' }} onPatch={vi.fn()} onOpen={vi.fn()} onAsk={vi.fn()} />);
  await userEvent.click(await screen.findByRole('button', { name: 'Try again' }));
  await screen.findByText('37 on the map · 36 relationships');
  expect(request).toHaveBeenLastCalledWith({ loupistId: 'loupist-quality', maxNodes: 1000 });
});
it('opens the key strip with its first-visit lead, keeps unlinked principles off the canvas, and remembers a collapse', async () => {
  localStorage.clear();
  // A first visit opens the legend whatever the window size; a collapse is remembered.
  mockMapBox(1000, 640);
  await setup();
  const legend = document.querySelector('details.map-legend') as HTMLDetailsElement;
  expect(legend.open).toBe(true);
  expect(screen.getByText(hintCopy)).toBeVisible();
  expect(document.querySelector('.map-hint-row')).toBeNull();
  const kinds = screen.getByTestId('map-canvas').getAttribute('data-kinds') ?? '';
  const edges = screen.getByTestId('map-canvas').getAttribute('data-edges') ?? '';
  expect(kinds).not.toContain('lens_');
  expect(edges).not.toContain('map-application');
  expect(edges).not.toContain('map-lens-link');
  legend.open = false;
  fireEvent(legend, new Event('toggle', { bubbles: true }));
  cleanup();
  await setup();
  expect((document.querySelector('details.map-legend') as HTMLDetailsElement).open).toBe(false);
  expect(screen.getByText(hintCopy)).not.toBeVisible();
  expect(document.querySelector('.map-first-hint')).toBeNull();
  vi.restoreAllMocks();
});
it('turns numbers back on when search selects a hidden metric', async () => {
  const { user } = await setup();
  const canvas = () => screen.getByTestId('map-canvas');
  const edges = () => (canvas().getAttribute('data-edges') ?? '').split(',').filter(Boolean);
  expect(canvas().getAttribute('data-nodes')).toContain('entity|Metric');
  const shownEdges = edges().length;
  await user.click(screen.getByRole('button', { name: 'Filter' }));
  const toggle = screen.getByRole('checkbox', { name: 'Show numbers and metrics' });
  expect(toggle).toBeChecked();
  await user.click(toggle);
  expect(canvas().getAttribute('data-nodes')).not.toContain('|Metric');
  expect(edges().length).toBeLessThan(shownEdges);
  await user.type(screen.getByRole('combobox'), 'Operating margin');
  await user.click(await screen.findByRole('option', { name: /Operating margin/ }));
  expect(await screen.findByRole('heading', { name: 'Operating margin' })).toBeVisible();
  expect(canvas().getAttribute('data-nodes')).toContain('entity|Metric');
  await user.click(screen.getByRole('button', { name: 'Filter' }));
  expect(screen.getByRole('checkbox', { name: 'Show numbers and metrics' })).toBeChecked();
});
it('reads the World through a Lens and clears back to the World', async () => {
  const { user } = await setup();
  const canvas = () => screen.getByTestId('map-canvas');
  await user.click(screen.getByRole('button', { name: 'Filter' }));
  await user.click(screen.getByText('Highlight Lens applications'));
  expect(screen.getByTestId('map-stats')).not.toHaveTextContent('World');
  expect(screen.getByText('Choose a Lens to highlight the companies, assets and other names it was applied to.')).toBeVisible();
  await user.click(screen.getByRole('button', { name: /Evidence before conviction/ }));
  expect(await screen.findByText(/hasn't been applied to anything yet/)).toBeVisible();
  expect(canvas()).toHaveAttribute('data-overlay', '');
  expect(canvas().getAttribute('data-kinds')).not.toContain('lens_');
  await user.click(screen.getByRole('button', { name: 'Filter' }));
  await user.click(screen.getByText('Highlight Lens applications'));
  await user.click(screen.getByRole('button', { name: /Compare competitive advantages/ }));
  const pill = await screen.findByText('Applied Lens: Compare competitive advantages');
  expect(pill.closest('[data-lens-applied]')).toHaveAttribute('data-lens-applied', 'map-lens-1');
  expect(canvas()).toHaveAttribute('data-overlay', 'map-entity-00,map-entity-02,map-entity-04,map-entity-06,map-entity-08');
  expect(screen.getByRole('button', { name: 'Apple' })).toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Filter' }));
  await user.click(screen.getByText('Highlight Lens applications'));
  await user.click(screen.getByRole('button', { name: 'No highlight' }));
  expect(screen.queryByText('Applied Lens: Compare competitive advantages')).not.toBeInTheDocument();
  expect(canvas()).toHaveAttribute('data-overlay', '');
  expect(await screen.findByText('37 on the map · 36 relationships')).toBeVisible();
});
it('shows the host default title as the localized perspective', async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const catalog = await bridge.knowledgeCatalog({ loupistId: 'loupist-quality' });
  const map = await bridge.knowledgeMap({ loupistId: catalog.loupistId });
  const standing = map.nodes.find(node => node.id === 'map-lens-0');
  standing!.label = 'Outlook';
  catalog.lenses.push({ ...catalog.lenses[0]!, recordId: 'map-lens-0', title: 'Outlook', lensKind: 'perspective', applicability: 'This Loupist has not recorded a standing philosophy.', isStanding: true } as typeof catalog.lenses[number]);
  vi.spyOn(bridge, 'knowledgeMap').mockResolvedValue(map);
  function Harness() {
    const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: catalog.loupistId, surface: 'world', recordId: catalog.worlds[0]?.recordId, revisionId: catalog.worlds[0]?.revisionId });
    return <MapView bridge={bridge} catalog={catalog} location={location} onPatch={setLocation} onOpen={vi.fn()} onAsk={vi.fn()} />;
  }
  render(<Harness />);
  await screen.findByText('37 on the map · 36 relationships');
  const user = userEvent.setup();
  await user.click(screen.getByRole('button', { name: 'Filter' }));
  await user.click(screen.getByText('Highlight Lens applications'));
  expect(screen.getByRole('button', { name: /^Perspective/ })).toBeVisible();
  expect(screen.queryByRole('button', { name: /Standing/ })).toBeNull();
  await user.keyboard('{Escape}');
  await user.type(screen.getByRole('combobox'), 'Standing');
  expect(await screen.findByText('No match on the map. Try Sources in the Library.')).toBeVisible();
  expect(screen.queryByRole('option')).toBeNull();
});
it('highlights the companies a Lens was applied to in Korean', async () => {
  setPreviewLanguage('ko');
  try {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const catalog = await bridge.knowledgeCatalog({ loupistId: 'loupist-quality' });
    function Harness() {
      const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: catalog.loupistId, surface: 'world', recordId: catalog.worlds[0]?.recordId, revisionId: catalog.worlds[0]?.revisionId });
      return <MapView bridge={bridge} catalog={catalog} location={location} onPatch={setLocation} onOpen={vi.fn()} onAsk={vi.fn()} />;
    }
    render(<Harness />);
    expect(await screen.findByTestId('map-stats')).not.toHaveTextContent('World');
    expect(screen.queryByRole('button', { name: '렌즈 위키' })).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: '필터' }));
    await userEvent.click(screen.getByText('렌즈 적용 대상 강조'));
    expect(screen.getByText('렌즈를 선택하면 이 관점으로 살펴본 기업·자산 등이 지도에서 강조돼요.')).toBeVisible();
    await userEvent.click(screen.getByRole('button', { name: /Compare competitive advantages/ }));
    expect(await screen.findByText('적용된 렌즈: Compare competitive advantages')).toBeVisible();
    expect(document.querySelector('[data-lens-applied="map-lens-1"]')).toBeTruthy();
    expect(screen.getByTestId('map-canvas')).toHaveAttribute('data-overlay', 'map-entity-00,map-entity-02,map-entity-04,map-entity-06,map-entity-08');
  } finally {
    setPreviewLanguage('en');
  }
});
it("finds an entity by a local name and shows that name in Korean", async () => {
  setPreviewLanguage("ko");
  try {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
    const map = await bridge.knowledgeMap({ loupistId: catalog.loupistId });
    const apple = map.nodes.find(node => node.label === "Apple");
    expect(apple).toBeTruthy();
    apple!.localNames = ["사과전자"];
    vi.spyOn(bridge, "knowledgeMap").mockResolvedValue(map);
    function Harness() {
      const [location, setLocation] = useState<KnowledgeLocation>({ loupistId: catalog.loupistId, surface: "world", recordId: catalog.worlds[0]?.recordId, revisionId: catalog.worlds[0]?.revisionId });
      return <MapView bridge={bridge} catalog={catalog} location={location} onPatch={setLocation} onOpen={vi.fn()} onAsk={vi.fn()} />;
    }
    render(<Harness />);
    await userEvent.type(await screen.findByRole("combobox"), "사과");
    const option = await screen.findByRole("option", { name: /사과전자/ });
    expect(option).toBeVisible();
    expect(screen.queryByRole("option", { name: /Apple/ })).toBeNull();
  } finally {
    setPreviewLanguage("en");
  }
});

it("keeps the World readable as a list when its canvas throws", async () => {
  canvasFailure.active = true;
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  try {
    await setup();
    expect(await screen.findByRole("table", { name: "List view" })).toBeVisible();
  } finally { canvasFailure.active = false; error.mockRestore(); }
});
