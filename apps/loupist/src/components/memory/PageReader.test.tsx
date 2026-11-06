import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { TooltipProvider } from "../ui/tooltip";
import { PageReader } from "./PageReader";

async function open(recordId = "lens-subject-tcp", tweak?: (bridge: MockLoupistBridge) => void, pageName = "Page") {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  tweak?.(bridge);
  const handlers = { onOpen: vi.fn(), onOpenPage: vi.fn(), onAsk: vi.fn(), onChanged: vi.fn() };
  render(<TooltipProvider><PageReader bridge={bridge} loupistId={loupistId} recordId={recordId} catalog={await bridge.knowledgeCatalog({ loupistId })} {...handlers} /></TooltipProvider>);
  return { bridge, loupistId, handlers, user: userEvent.setup(), page: await screen.findByRole("article", { name: pageName }) };
}

it("reads sections in order, each with its basis chip, and marks a contested one", async () => {
  const { page, handlers, loupistId, user } = await open();
  expect(within(page).getAllByRole("heading", { level: 3 }).slice(0, 4).map(heading => heading.textContent)).toEqual(["Overview", "Facts you told me", "Where it is heading", "Risks"]);
  const section = (key: string) => page.querySelector<HTMLElement>(`[data-section-key="${key}"]`)!;
  expect(within(section("overview")).getByRole("button", { name: "Claim" })).toBeVisible();
  await user.click(within(section("overview")).getByRole("button", { name: "Source" }));
  expect(handlers.onOpen).toHaveBeenCalledWith(expect.objectContaining({ loupistId, surface: "source", recordId: "source-pack", selection: expect.objectContaining({ kind: "source", spanId: "span-status-codes" }) }));
  expect(within(section("facts")).getByText("You said")).toBeVisible();
  expect(within(section("views")).getByText("Loupist’s view")).toBeVisible();
  expect(within(section("views")).getByText(/Not sure: Based on one analyst report/)).toBeVisible();
  const contested = within(section("risks")).getByRole("button", { name: "Sources disagree" });
  expect(section("risks")).toHaveAttribute("data-contested");
  await user.click(contested);
  expect(handlers.onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ surface: "world", selection: { kind: "claim", id: "claim-quic-blocked" } }));
  expect(section("overview")).not.toHaveAttribute("data-contested");
});

it("forgets a section through the bridge and brings it back with Undo", async () => {
  const { page, bridge, loupistId, handlers, user } = await open();
  const forget = vi.spyOn(bridge, "memorySectionForget"), undo = vi.spyOn(bridge, "memoryOpUndo");
  await user.click(within(page).getByRole("button", { name: "Forget Risks" }));
  expect(forget).toHaveBeenCalledWith({ loupistId, recordId: "lens-subject-tcp", key: "risks" });
  expect(await within(page).findByRole("status")).toHaveTextContent("Forgot “Risks”.");
  expect(within(page).queryByRole("heading", { name: "Risks" })).toBeNull();
  await user.click(within(page).getByRole("button", { name: "Undo" }));
  expect(undo).toHaveBeenCalledWith({ loupistId, opId: expect.any(String) });
  expect(await within(page).findByRole("heading", { name: "Risks" })).toBeVisible();
  expect(handlers.onChanged).toHaveBeenCalledTimes(2);
});

it("opens a linked page and a Map name, and lists a short history", async () => {
  const { page, handlers, user } = await open("lens-subject-tcp", bridge => {
    const read = bridge.knowledgeLensRead.bind(bridge);
    vi.spyOn(bridge, "knowledgeLensRead").mockImplementation(async request => {
      const view = await read(request);
      if (view.body.kind === "principle") return view;
      view.body.sections[0] = { ...view.body.sections[0]!, markdown: "Compared with [QUIC](world:node-quic) and [the idea](lens:lens-idea-quic)." };
      return view;
    });
  });
  await user.click(within(page).getByRole("link", { name: "QUIC" }));
  expect(handlers.onOpen).toHaveBeenCalledWith(expect.objectContaining({ surface: "world", selection: { kind: "node", id: "node-quic" } }));
  await user.click(within(page).getByRole("link", { name: "the idea" }));
  expect(handlers.onOpenPage).toHaveBeenCalledWith("lens-idea-quic");
  await user.click(within(within(page).getByRole("region", { name: "On the map" })).getByRole("button", { name: "TCP" }));
  expect(handlers.onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ selection: { kind: "node", id: "node-tcp" } }));
  expect(within(within(page).getByRole("region", { name: "History" })).getAllByRole("listitem").map(item => item.textContent)).toEqual([expect.stringContaining("Added “Risks”"), expect.stringContaining("Added “Overview”")]);
});

it("keeps an idea's status and a principle's own fields", async () => {
  const idea = await open("lens-idea-quic");
  expect(within(idea.page).getByText("Not checked yet")).toBeVisible();
});

it("shows the host-default perspective title and English default headings in Korean", async () => {
  const { default: i18n } = await import("@/i18n");
  await i18n.changeLanguage("ko");
  try {
    const { page } = await open("lens-perspective", bridge => {
      const read = bridge.knowledgeLensRead.bind(bridge);
      vi.spyOn(bridge, "knowledgeLensRead").mockImplementation(async request => {
        const view = await read(request);
        if (view.body.kind === "principle") return view;
        view.title = "Outlook";
        view.body.sections[0] = { ...view.body.sections[0]!, key: "beliefs", heading: "Beliefs" };
        return view;
      });
    }, "페이지");
    expect(within(page).getByRole("heading", { level: 2 })).toHaveTextContent("관점");
    expect(within(page).getByRole("heading", { name: "신념" })).toBeVisible();
    expect(within(page).queryByText("Outlook")).toBeNull();
    expect(within(page).queryByRole("heading", { name: "Beliefs" })).toBeNull();
  } finally {
    await i18n.changeLanguage("en");
  }
});
