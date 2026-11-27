import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { TooltipProvider } from "../ui/tooltip";
import { RequestTurnProvider } from "./requestTurn";
import { PageReader } from "./PageReader";
import { LearnRequestProvider } from "../briefings/LearnRequest";

async function open(recordId = "lens-subject-tcp", tweak?: (bridge: MockLoupistBridge) => void, pageName = "Page") {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  tweak?.(bridge);
  const handlers = { onOpen: vi.fn(), onOpenPage: vi.fn(), onAsk: vi.fn(), onChanged: vi.fn() };
  render(<TooltipProvider><PageReader bridge={bridge} loupistId={loupistId} recordId={recordId} catalog={await bridge.knowledgeCatalog({ loupistId })} {...handlers} /></TooltipProvider>);
  return { bridge, loupistId, handlers, user: userEvent.setup(), page: await screen.findByRole("article", { name: pageName }) };
}

it("reads sections in order, with their sources and other basis, and marks a contested one", async () => {
  const { page, handlers, loupistId, user } = await open();
  expect(within(page).getAllByRole("heading", { level: 3 }).slice(0, 4).map(heading => heading.textContent)).toEqual(["Overview", "Facts you told me", "Where it is heading", "Risks"]);
  const section = (key: string) => page.querySelector<HTMLElement>(`[data-section-key="${key}"]`)!;
  expect(within(section("overview")).getByRole("button", { name: "Claim" })).toBeVisible();
  const overview = section("overview");
  const summary = within(overview).getByText("Sources 1").closest("summary")!;
  const text = within(overview).getByText(/^TCP carries most web traffic/);
  expect(within(overview).getByRole("heading").compareDocumentPosition(text) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(text.compareDocumentPosition(summary) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(summary.parentElement).not.toHaveAttribute("open");
  expect(section("facts").querySelector("details")).toBeNull();
  await user.click(summary);
  const source = within(overview).getByRole("link", { name: /RFC 9110 HTTP semantics/ });
  expect(source).toHaveTextContent("IETF RFC 9110");
  await user.click(source);
  expect(handlers.onOpen).toHaveBeenCalledWith(expect.objectContaining({ loupistId, surface: "source", recordId: "source-pack", selection: expect.objectContaining({ kind: "source", spanId: "span-status-codes" }) }));
  expect(within(section("facts")).getByText(/Written at your request/)).toBeVisible();
  expect(within(section("views")).getByText("Loupist’s view")).toBeVisible();
  expect(within(section("views")).getByText(/Not sure: Based on one analyst report/)).toBeVisible();
  // A page chip may be shortened on screen; its name and tooltip keep the whole title.
  const pageChip = within(section("views")).getByRole("button", { name: /^From / });
  expect(pageChip.textContent!.length).toBeGreaterThan("From ".length);
  expect(pageChip).toHaveAttribute("title", pageChip.textContent);
  const contested = within(section("risks")).getByRole("button", { name: "Sources disagree" });
  expect(section("risks")).toHaveAttribute("data-contested");
  await user.click(contested);
  expect(handlers.onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ surface: "world", selection: { kind: "claim", id: "claim-quic-blocked" } }));
  expect(section("overview")).not.toHaveAttribute("data-contested");
});

it("marks superseded and contested sections, a role and an as-of date, and counts sections on changed facts", async () => {
  const scroll = vi.fn();
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, writable: true, value: scroll });
  const { page, handlers, user } = await open(undefined, bridge => {
    const read = bridge.knowledgeLensRead.bind(bridge);
    vi.spyOn(bridge, "knowledgeLensRead").mockImplementation(async request => {
      const view = await read(request);
      if (view.body.kind === "principle") return view;
      view.body.sections = view.body.sections.map((item, index) => {
        if (index === 0) return { ...item, role: "check" as const, asOf: "2024-03-31", contested: false, contestedClaimIds: [], sinceMs: 60, claimIds: ["prior-claim"] };
        if (index === 1) return { ...item, role: "open_question" as const, contested: true, sinceMs: 70, claimIds: ["also"], contestedClaimIds: ["kept-claim"] };
        if (index === 2) return { ...item, role: "refutation" as const, asOf: "2024", contested: false, contestedClaimIds: [] };
        return { ...item, role: "fact" as const, contested: false, contestedClaimIds: [], sinceMs: undefined, claimIds: undefined };
      });
      return view;
    });
  });
  await user.click(within(page).getByRole("button", { name: "2 sections rest on facts that changed" }));
  expect(scroll).toHaveBeenCalled();
  const superseded = page.querySelector<HTMLElement>('[data-section-state="superseded"]')!;
  expect(within(superseded).getByText("Check")).toBeVisible();
  expect(within(superseded).getByText(/^As of /)).toBeVisible();
  expect(within(superseded).getByRole("button", { name: "Superseded" })).toBeVisible();
  expect(within(superseded).queryByRole("button", { name: "Sources disagree" })).toBeNull();
  const contested = page.querySelector<HTMLElement>('[data-section-state="contested"]')!;
  expect(within(contested).getByText("Open question")).toBeVisible();
  expect(within(contested).getByRole("button", { name: "Sources disagree" })).toBeVisible();
  expect(within(contested).queryByRole("button", { name: "Superseded" })).toBeNull();
  expect(within(page).getByText("Would break it")).toBeVisible();
  expect(page.querySelectorAll(".page-section-role")).toHaveLength(3);
  await user.click(within(superseded).getByRole("button", { name: "Superseded" }));
  expect(handlers.onOpen).toHaveBeenCalledWith(expect.objectContaining({ surface: "world", selection: { kind: "claim", id: "prior-claim" } }));
});

it("names the source that ended a superseded section once its history is read", async () => {
  const { page } = await open(undefined, bridge => {
    vi.spyOn(bridge, "knowledgeWorldClaimHistory").mockResolvedValue({
      created: null, supported: [], endedClaims: [], endedByClaimId: null, endings: [],
      ended: { sourceTitle: "Later note", end: { value: "2024-06-01", precision: "date" }, readArgs: { recordId: "rec" } },
    });
    const read = bridge.knowledgeLensRead.bind(bridge);
    vi.spyOn(bridge, "knowledgeLensRead").mockImplementation(async request => {
      const view = await read(request);
      if (view.body.kind === "principle") return view;
      view.body.sections = view.body.sections.map((item, index) => index === 0
        ? { ...item, contested: false, contestedClaimIds: [], sinceMs: 60, claimIds: ["prior-claim"] }
        : item);
      return view;
    });
  });
  const superseded = page.querySelector<HTMLElement>('[data-section-state="superseded"]')!;
  expect(await within(superseded).findByRole("button", { name: /Superseded · .+ · Later note/ })).toBeVisible();
});

it("links a method's request to the dated user turn without attributing the method to the user", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const view = await bridge.knowledgeLensRead({ loupistId, recordId: "lens-subject-tcp" });
  if (view.body.kind === "principle") throw new Error("Expected a sectioned page");
  vi.spyOn(bridge, "knowledgeLensRead").mockResolvedValue({ ...view, kind: "method", body: { ...view.body, kind: "method" } });
  const jump = vi.fn();
  render(<TooltipProvider><RequestTurnProvider value={id => id === "mock-user-message" ? { date: "2026-11-19T12:00:00Z", open: jump } : undefined}>
    <PageReader bridge={bridge} loupistId={loupistId} recordId={view.recordId} onOpen={vi.fn()} onOpenPage={vi.fn()} />
  </RequestTurnProvider></TooltipProvider>);
  await userEvent.click(await screen.findByRole("link", { name: "Written at your request · Nov 19" }));
  expect(jump).toHaveBeenCalledOnce();
  expect(screen.queryByText("You said")).toBeNull();
});

it("lists complete, similar titles in citation order", async () => {
  const titles = ["Net sales by segment", "Cash flow from operations"].map(part => `Annual report and consolidated financial statements — ${part}`);
  const { page, user } = await open(undefined, bridge => {
    const catalog = bridge.knowledgeCatalog.bind(bridge);
    vi.spyOn(bridge, "knowledgeCatalog").mockImplementation(async request => {
      const result = await catalog(request);
      result.sources = titles.map((title, i) => ({ ...result.sources[0]!, recordId: `report-${i}`, title, objects: [{ id: `source-${i}`, title }] }));
      return result;
    });
    const read = bridge.knowledgeLensRead.bind(bridge);
    vi.spyOn(bridge, "knowledgeLensRead").mockImplementation(async request => {
      const view = await read(request);
      if (view.body.kind !== "principle") view.body.sections[0]!.basis.refs = titles.map((_, i) => ({
        kind: "span", recordId: `report-${i}`, spanId: `span-${i}`, revisionDigest: "revision",
      }));
      return view;
    });
  });
  const overview = within(page).getByRole("region", { name: "Overview" });
  await user.click(within(overview).getByText("Sources 2"));
  expect(within(overview).getAllByRole("link").map(link => link.querySelector("strong")?.textContent)).toEqual(titles);
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

it("opens a linked page and a World name, and lists a short history", async () => {
  const { page, handlers, user } = await open("lens-subject-tcp", bridge => {
    const read = bridge.knowledgeLensRead.bind(bridge);
    vi.spyOn(bridge, "knowledgeLensRead").mockImplementation(async request => {
      const view = await read(request);
      if (view.body.kind === "principle") return view;
      view.body.sections[0] = { ...view.body.sections[0]!, markdown: "Compared with [QUIC](world:node-quic) and [the idea](lens:lens-idea-quic)." };
      view.body.sections[1] = { ...view.body.sections[1]!, heading: "   " };
      view.history[0] = { ...view.history[0]!, sectionHeading: "Original risk assessment" };
      return view;
    });
  });
  expect(page.querySelector('[data-section-key="facts"] h3')).toBeNull();
  await user.click(within(page).getByRole("link", { name: "QUIC" }));
  expect(handlers.onOpen).toHaveBeenCalledWith(expect.objectContaining({ surface: "world", selection: { kind: "node", id: "node-quic" } }));
  await user.click(within(page).getByRole("link", { name: "the idea" }));
  expect(handlers.onOpenPage).toHaveBeenCalledWith("lens-idea-quic");
  await user.click(within(within(page).getByRole("region", { name: "In World" })).getByRole("button", { name: "TCP" }));
  expect(handlers.onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ selection: { kind: "node", id: "node-tcp" } }));
  expect(within(within(page).getByRole("region", { name: "History" })).getAllByRole("listitem").map(item => item.textContent)).toEqual([expect.stringContaining("Added “Original risk assessment”"), expect.stringContaining("Added “Overview”")]);
});

it("keeps an idea's status and a principle's own fields", async () => {
  const idea = await open("lens-idea-quic");
  expect(within(idea.page).getByText("Not checked yet")).toBeVisible();
});

it("shows English default headings in Korean and the perspective title as written", async () => {
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
    expect(within(page).getByRole("heading", { level: 2 })).toHaveTextContent(/^Outlook$/);
    expect(within(page).getByRole("heading", { name: "신념" })).toBeVisible();
    expect(within(page).queryByRole("heading", { name: "Beliefs" })).toBeNull();
  } finally {
    await i18n.changeLanguage("en");
  }
});

it("offers Keep this page current on a subject page only, where the app can ask Bot (plan 4.4)", async () => {
  const ask = vi.fn();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  const reader = (recordId: string) => <TooltipProvider><LearnRequestProvider value={{ block: null, ask }}>
    <PageReader bridge={bridge} loupistId={loupistId} recordId={recordId} onOpen={vi.fn()} onOpenPage={vi.fn()} />
  </LearnRequestProvider></TooltipProvider>;
  const view = render(reader("lens-subject-tcp"));
  await userEvent.click(await screen.findByRole("button", { name: "Keep this page current" }));
  expect(ask).toHaveBeenCalledExactlyOnceWith("Propose a learning program that keeps my Lens page “TCP” current.");
  view.rerender(reader("lens-idea-quic"));
  await screen.findByRole("heading", { level: 2, name: "QUIC could displace TCP for mobile traffic" });
  expect(screen.queryByRole("button", { name: "Keep this page current" })).toBeNull();
});
