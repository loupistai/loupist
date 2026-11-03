import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { KnowledgeLocation, LensProposal, LensView } from "../../types";
import { LensWorkspace } from "./LensWorkspace";

async function fixture() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
  const method = catalog.lenses.find((item) => item.lensKind === "method")!;
  const lens = await bridge.knowledgeLensRead({ loupistId: catalog.loupistId, recordId: method.recordId });
  const location: KnowledgeLocation = { loupistId: catalog.loupistId, surface: "lens", recordId: method.recordId };
  const props = { bridge, catalog, location, onOpen: vi.fn(), onPatch: vi.fn(), onAsk: vi.fn(), onChanged: vi.fn() };
  return { props, lens };
}
const proposalFor = (lens: LensView, id = "proposal-a"): LensProposal => ({ draftId: id, loupistId: "loupist-quality", author: "loupist", recordId: lens.recordId, baseRevisionId: lens.revisionId, document: { ...lens.document, markdown: `Proposed method ${id}.` }, rationale: "New evidence calls for a revision.", updatedAtMs: 1, originChatId: "origin-chat", originMessageId: id });

it("reads the saved Lens and attaches its exact version to Chat", async () => {
  const { props, lens } = await fixture();
  render(<LensWorkspace {...props} />);
  await screen.findByRole("heading", { name: lens.title });
  expect(screen.queryByRole("button", { name: /Edit method|Edit perspective|Lens actions|Add method|Save Lens|Rename|Archive/ })).toBeNull();
  expect(screen.queryByRole("textbox", { name: /Method|Change reason/ })).toBeNull();
  await userEvent.setup().click(screen.getByRole("button", { name: "Ask Bot to revise" }));
  expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ kind: "lens", recordId: lens.recordId, revisionId: lens.revisionId }));
});

it("reports an unavailable saved identity without presenting a historical document", async () => {
  const { props } = await fixture();
  render(<LensWorkspace {...props} location={{ ...props.location, revisionId: "superseded" }} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Lens revision not found.");
  expect(screen.queryByRole("article", { name: "Lens reader" })).toBeNull();
});

it("keeps a late read from replacing the selected Lens", async () => {
  const { props, lens } = await fixture();
  let resolve!: (view: LensView) => void;
  const read = props.bridge.knowledgeLensRead.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeLensRead").mockImplementationOnce(() => new Promise((done) => { resolve = done; })).mockImplementation(read);
  const view = render(<LensWorkspace {...props} />);
  const other = props.catalog.lenses.find((item) => item.recordId !== lens.recordId)!;
  view.rerender(<LensWorkspace {...props} location={{ ...props.location, recordId: other.recordId }} />);
  await screen.findByRole("heading", { name: other.title });
  await act(async () => resolve(lens));
  expect(screen.queryByRole("heading", { name: lens.title })).toBeNull();
});

it("retries failed reads and saved basis without losing the Lens", async () => {
  const { props, lens } = await fixture();
  const read = props.bridge.knowledgeLensRead.bind(props.bridge);
  vi.spyOn(props.bridge, "knowledgeLensRead").mockRejectedValueOnce(new Error("Temporarily unavailable")).mockImplementation(read);
  vi.spyOn(props.bridge, "knowledgeLensChanges").mockRejectedValueOnce(new Error("Saved basis unavailable")).mockResolvedValue([]);
  render(<LensWorkspace {...props} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Retry Lens" }));
  await screen.findByRole("heading", { name: lens.title });
  await user.click(screen.getByText("Saved basis"));
  await user.click(await screen.findByRole("button", { name: "Retry saved basis" }));
  expect(await screen.findByText("No saved basis.")).toBeVisible();
});

it("reads the exact proposal requested from Chat and returns to its origin for discussion", async () => {
  const { props, lens } = await fixture();
  const proposal = proposalFor(lens);
  vi.spyOn(props.bridge, "knowledgeLensProposals").mockResolvedValue([proposalFor(lens, "other"), proposal]);
  render(<LensWorkspace {...props} location={{ ...props.location, view: { reviewProposal: { chatId: "origin-chat", messageId: "proposal-a" } } }} />);
  const region = await screen.findByRole("region", { name: "Proposal 1 of 1" });
  expect(region).toHaveTextContent("Proposed method proposal-a.");
  expect(region).not.toHaveTextContent("Proposed method other.");
  expect(screen.queryByRole("form", { name: "Lens editor" })).toBeNull();
  await userEvent.setup().click(within(region).getByRole("button", { name: "Discuss proposal in Chat" }));
  expect(props.onAsk).toHaveBeenCalledWith(expect.objectContaining({ recordId: lens.recordId }), "origin-chat");
});

it("dismisses a proposal only once and allows retry after a failure", async () => {
  const { props, lens } = await fixture();
  vi.spyOn(props.bridge, "knowledgeLensProposals").mockResolvedValue([proposalFor(lens)]);
  let reject!: (cause: Error) => void;
  const dismiss = vi.spyOn(props.bridge, "knowledgeLensProposalDismiss").mockImplementationOnce(() => new Promise((_, fail) => { reject = fail; })).mockResolvedValue(undefined);
  render(<LensWorkspace {...props} />);
  const user = userEvent.setup();
  await user.click(await screen.findByText("Proposed changes 1"));
  await user.dblClick(screen.getByRole("button", { name: "Dismiss" }));
  expect(dismiss).toHaveBeenCalledOnce();
  await act(async () => reject(new Error("Try again later")));
  expect(await screen.findByRole("alert")).toHaveTextContent("Try again later");
  await user.click(screen.getByRole("button", { name: "Dismiss" }));
  await waitFor(() => expect(screen.queryByText("Proposed changes 1")).toBeNull());
  expect(props.onChanged).toHaveBeenCalledOnce();
});
