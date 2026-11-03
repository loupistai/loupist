import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { WorldGraphView } from "../../contracts/knowledge";
import { mainTab, openApp } from "../renderApp";

async function openMap(bridge = new MockLoupistBridge({ delay_ms: 0 })) {
  const user = userEvent.setup();
  await openApp(bridge);
  await user.click(await mainTab(/Map/));
  await screen.findByRole("combobox", { name: /Search the map/ });
  await screen.findByText("37 on the map · 36 relationships");
  return { user, bridge };
}

describe("Loupist · Map", () => {
  it("browses the unified map through its keyboard-accessible list and preserves selection across Chat", async () => {
    const { user } = await openMap();
    await user.click(screen.getByRole("button", {name:"More actions"}));
    await user.click(screen.getByRole("menuitemcheckbox", {name:"List view"}));
    const names = screen.getByRole("table", { name: "List view" });
    expect(within(names).getAllByRole("row")).toHaveLength(38);
    await user.click(within(names).getByRole("button", { name: "Apple" }));
    expect(await screen.findByRole("heading", { name: "Apple" })).toBeVisible();
    await user.click(await mainTab(/Chat/));
    expect(screen.getByRole("textbox", { name: "Message Loupist" })).toBeVisible();
    await user.click(await mainTab(/Map/));
    expect(await screen.findByRole("heading", { name: "Apple" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "More actions" }));
    await user.click(screen.getByRole("menuitemcheckbox", { name: "Map view" }));
    expect(await screen.findByRole("img", { name: "37 on the map · 36 relationships" })).toBeVisible();
  });

  it("opens the Lens wiki independently of the map and shows the saved body and method fields as Markdown", async () => {
    const { user, bridge } = await openMap();
    const loupistId = "loupist-quality";
    const catalog = await bridge.knowledgeCatalog({ loupistId });
    const method = catalog.lenses.find(lens => lens.lensKind === "method")!;
    const saved = await bridge.knowledgeLensRead({ loupistId, recordId: method.recordId });
    expect(screen.queryByRole("button", { name: "Lens wiki" })).toBeNull();
    await user.click(await mainTab(/Lens/));
    const page = await screen.findByRole("region", {name:"Research principles"});
    await user.click(await within(page).findByRole("tab", { name:/Ways of working/ }));
    await user.click(await within(page).findByRole("button", { name: new RegExp(method.title) }));
    const article = await within(page).findByRole("article", { name: "Lens reader" });
    await within(article).findByRole("heading", { name: method.title });
    await user.click(within(article).getByText("View Markdown"));
    const markdown = await waitFor(() => {
      const source = article.querySelector("pre");
      expect(source).toBeVisible();
      return source!.textContent!;
    });
    expect(markdown.startsWith(saved.document.markdown)).toBe(true);
    expect(markdown).toContain(`## Procedure\n\n${saved.document.procedure}`);
    await user.click(await mainTab(/Map/));
    expect(screen.getByRole("combobox", { name: "Search the map" })).toBeVisible();
    await user.click(await mainTab(/Lens/));
    expect(await screen.findByRole("region", {name:"Research principles"})).toBeVisible();
  });

  it("ignores a late remote alias search when the query changes", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const original = bridge.knowledgeWorldQuery.bind(bridge);
    let release: ((page: WorldGraphView) => void) | undefined;
    vi.spyOn(bridge, "knowledgeWorldQuery").mockImplementation((request) => request.search === "Ghost" ? new Promise((resolve) => { release = resolve; }) : original(request));
    const { user } = await openMap(bridge);
    const search = screen.getByRole("combobox", { name: "Search the map" });
    await user.type(search, "Ghost");
    await waitFor(() => expect(release).toBeDefined());
    await user.clear(search);
    await user.type(search, "Apple");
    await screen.findByRole("option", { name: /Apple/ });
    const page = await original({ loupistId: 'loupist-quality', recordId: 'world-protocols', revisionId: 'rev-owned-1', search: '', kinds: [], relations: [], statuses: [], bindingRoles: [], nodeLimit: 8, edgeLimit: 8, byteLimit: 524288 });
    await act(async () => release!({ ...page, nodes: [{ id: 'ghost', name: 'Ghost', aliases: [], kind: 'Entity', description: '', sourceRole: '', properties: {} }] }));
    expect(screen.queryByRole("option", { name: /Ghost/ })).not.toBeInTheDocument();
    expect(screen.getByRole("option", { name: /Apple/ })).toBeVisible();
  });

  it("opens the exact historical Source from an entity fact and preserves its map search", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const query = bridge.knowledgeWorldQuery.bind(bridge);
    vi.spyOn(bridge, "knowledgeWorldQuery").mockImplementation(async (request) => ({ ...await query(request), sourceTargets: {
      "span-status-codes": { recordId: "source-historical", revisionId: "source-old", sourceId: "source-rfc-9110", spanId: "span-status-codes", worldRecordId: request.recordId, worldRevisionId: request.revisionId },
    } }));
    const read = bridge.knowledgeSourceRead.bind(bridge);
    const readSpy = vi.spyOn(bridge, "knowledgeSourceRead").mockImplementation(async (request) => ({ ...await read({ ...request, recordId: "source-pack", revisionId: "rev-source-1" }), recordId: request.recordId, revisionId: request.revisionId }));
    const { user } = await openMap(bridge);
    await user.type(screen.getByRole("combobox"), "HTTP");
    await user.click(await screen.findByRole("option", { name: /^HTTP/ }));
    const inspector = await screen.findByRole("complementary", { name: "HTTP" });
    await user.click((await within(inspector).findAllByRole("button", { name: "Open source" }))[0]);
    await waitFor(() => expect(readSpy).toHaveBeenCalledWith(expect.objectContaining({ recordId: "source-historical", revisionId: "source-old", sourceId: "source-rfc-9110", spanId: "span-status-codes", worldRecordId: "world-protocols", worldRevisionId: "rev-owned-1" })));
    expect(await screen.findByRole("article", { name: "Source reader" })).toHaveTextContent("The status-code element is a three-digit integer code.");
  });
});
