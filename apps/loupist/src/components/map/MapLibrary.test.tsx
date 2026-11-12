import { useState } from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { initialKnowledgeNavigation, openKnowledgeLocation, patchKnowledgeLocation } from "../../knowledge/location";
import { MemoryWorkspace } from "../knowledge/MemoryWorkspace";
vi.mock('./MapCanvas', () => ({ default: ({ selectedId, overlayIds }: { selectedId?: string; overlayIds?: Set<string> }) => <div data-testid="map-canvas" data-selected={selectedId} data-overlay={[...(overlayIds ?? [])].join(',')} /> }));
async function setup() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupist = (await bridge.loupistList())[0];
  const catalog = await bridge.knowledgeCatalog({ loupistId: loupist.id });
  const method = catalog.lenses.find(l => l.lensKind === 'method')!;
  const perspective = catalog.lenses.find(l => l.lensKind === 'perspective')!;
  const map = await bridge.knowledgeMap({ loupistId: loupist.id });
  map.nodes = [map.nodes[0], { id: 'method', recordId: method.recordId, label: method.title, kind: 'lens_method', degree: 1, watched: false, held: false }, { id: 'perspective', recordId: perspective.recordId, label: perspective.title, kind: 'lens_perspective', degree: 0, watched: false, held: false }];
  map.edges = [{ id: 'application', source: 'method', target: map.nodes[0].id, kind: 'applies_to' }];
  vi.spyOn(bridge, 'knowledgeMap').mockResolvedValue(map);
  const read = vi.spyOn(bridge, 'knowledgeSourceRead');
  function Harness() {
    const [navigation, setNavigation] = useState(() => initialKnowledgeNavigation(loupist.id));
    return <MemoryWorkspace bridge={bridge} loupist={loupist} navigation={navigation} onPatch={next => setNavigation(n => patchKnowledgeLocation(n, next))} onOpen={next => setNavigation(n => openKnowledgeLocation(n, next))} onBack={vi.fn()} onAsk={vi.fn()} />;
  }
  const view = render(<Harness />);
  await screen.findByText('1 on the map · 0 relationships');
  return { user: userEvent.setup(), method, perspective, read, remount: () => { view.rerender(<></>); view.rerender(<Harness />); } };
}
it('opens a compact Source library and reads an exact source in the map inspector', async () => {
  const { user, read } = await setup();
  await user.click(screen.getByRole('button', { name: 'Library' }));
  const library = screen.getByRole('complementary', { name: 'Library' });
  expect(within(library).getByRole('button', { name: 'Add source' })).toBeVisible();
  await user.click(within(library).getByRole('button', { name: /RFC 9110 HTTP semantics/ }));
  const inspector = await screen.findByRole('complementary', { name: 'Document inspector' });
  expect(await within(inspector).findByRole('article', { name: 'Source reader' })).toBeVisible();
  expect(read).toHaveBeenCalledWith(expect.objectContaining({ sourceId: 'source-rfc-9110', purpose: 'renderer_inspection' }));
  expect(screen.getByTestId('map-canvas')).toBeInTheDocument();
  await user.click(within(inspector).getByRole('button', { name: 'Close inspector' }));
  expect(screen.queryByRole('complementary', { name: 'Document inspector' })).not.toBeInTheDocument();
});
it('applies only application neighbors, reports an unused Lens and exits with Escape', async () => {
  const { user, method, perspective } = await setup();
  await user.click(screen.getByRole('button', { name: 'Filter' }));
  await user.click(screen.getByText('Highlight Lens applications'));
  await user.click(screen.getByRole('button', { name: new RegExp(method.title) }));
  expect(screen.getByText(`Applied Lens: ${method.title}`)).toBeVisible();
  expect(screen.getByTestId('map-canvas')).toHaveAttribute('data-overlay', 'map-entity-00');
  await user.keyboard('{Escape}');
  expect(screen.queryByText(`Applied Lens: ${method.title}`)).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: 'Filter' }));
  await user.click(screen.getByText('Highlight Lens applications'));
  await user.click(screen.getByRole('button', { name: new RegExp(perspective.title) }));
  expect(screen.getByRole('button', { name: 'Ask in Chat' })).toBeVisible();
  expect(screen.getByTestId('map-canvas')).toHaveAttribute('data-overlay', '');
});
it('restores the Library tab and query after the map is remounted', async () => {
  const { user, remount } = await setup();
  await user.click(screen.getByRole('button', { name: 'Library' }));
  await user.type(screen.getByRole('textbox', { name: 'Search sources' }), 'RFC');
  remount();
  expect(await screen.findByRole('textbox', { name: 'Search sources' })).toHaveValue('RFC');
  await user.click(screen.getByRole('button', { name: 'More actions' }));
  expect(screen.getByRole("button", { name: "Library" })).toHaveAttribute("aria-expanded", "true");
});
