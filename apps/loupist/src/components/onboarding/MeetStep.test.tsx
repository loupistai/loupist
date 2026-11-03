import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { FollowedStatus, GrowthEvent, KnowledgeMap, WorldSchemaView } from "@/contracts/knowledge";
import type { LoupistBridge } from "@/types";
import { MeetStep } from "./MeetStep";

const schema = (proposal: WorldSchemaView["proposal"] = null): WorldSchemaView => ({ extension: { version: 1, types: [], relations: [], updatedAtMs: 0 }, proposal });
const added = (id: string, atMs: number): GrowthEvent => ({ eventId: `event-${id}`, loupistId: "loupist", atMs, kind: "entity_added", subjectId: id, recordId: "world", originChatId: null, originMessageId: null, actor: "import", summary: id, beforeSummary: null, afterSummary: null });
const map = (ids: string[]): KnowledgeMap => ({ nodes: ids.map(id => ({ id, label: id === "samsung" ? "Samsung Electronics" : "SK hynix", kind: "entity", entityKind: "Organization", degree: 0, watched: true, held: false })), edges: [], worldRevisionId: `rev-${ids.length}` } as unknown as KnowledgeMap);

/**
 * `followed` is Rust's job state for each read, in order; the last one repeats. By default the
 * read is running until the first names arrive, then done.
 */
function stub(options: { watching?: boolean; proposal?: WorldSchemaView["proposal"]; followed?: FollowedStatus[]; grows?: boolean } = {}) {
  let reads = 0;
  const grows = options.grows ?? true;
  const followed = options.followed ?? [{ state: "running", assets: 1, queuedAtMs: Date.now() }, { state: "done", assets: 1 }];
  let statuses = 0;
  const bridge = {
    watchlistList: vi.fn(async () => options.watching === false ? [] : [{ symbol: "005930", market: "KRX", displayName: "Samsung Electronics", addedAtMs: 0 }]),
    // The first read finds nothing; the map grows on the next ones.
    knowledgeGrowthEvents: vi.fn(async () => { reads += 1; return { events: !grows || reads === 1 ? [] : reads === 2 ? [added("samsung", Date.now())] : [added("samsung", Date.now() - 1000), added("hynix", Date.now())], nextCursor: null }; }),
    knowledgeMap: vi.fn(async () => map(!grows || reads <= 1 ? [] : reads === 2 ? ["samsung"] : ["samsung", "hynix"])),
    worldSchemaGet: vi.fn(async () => schema(options.proposal ?? null)),
    worldSchemaApply: vi.fn(async () => schema({ ...options.proposal!, status: "applied" })),
    worldSchemaDismiss: vi.fn(async () => schema({ ...options.proposal!, status: "dismissed" })),
    knowledgeFollowedStatus: vi.fn(async () => followed[Math.min(statuses++, followed.length - 1)]!),
  };
  return bridge;
}

function meet(bridge: ReturnType<typeof stub>, focus = "Memory makers") {
  const onFinish = vi.fn();
  render(<MeetStep bridge={bridge as unknown as LoupistBridge} loupistId="loupist" name="Chip desk" focus={focus} finishing={false} onFinish={onFinish} />);
  return onFinish;
}

beforeEach(() => { vi.useFakeTimers({ shouldAdvanceTime: true }); });
afterEach(() => { vi.useRealTimers(); });

it("reads what the user follows, then appends names from the growth log", async () => {
  const bridge = stub();
  const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
  const onFinish = meet(bridge);
  // Nothing has loaded yet, and the user can already go on.
  expect(screen.getByRole("button", { name: "Start chatting" })).toBeEnabled();
  const log = await screen.findByRole("log");
  // Rust says the World job reading the "What you follow" Source is running.
  expect(await within(log).findByText("Reading what you follow…")).toBeVisible();
  expect(within(log).getByText(/^Tracking: companies, products & technology, people/)).toBeVisible();
  expect(screen.getByText("Names appear here as your Loupist reads what you follow.")).toBeVisible();
  // With the question there is no Start button, so the caption says where both answers lead.
  expect(screen.getByText("Either choice opens your Loupist.")).toBeVisible();
  expect(screen.getByRole("button", { name: "Not now" })).toBeEnabled();
  await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
  expect(await within(log).findByText("Added Samsung Electronics")).toBeVisible();
  expect(within(log).queryByText("Reading what you follow…")).toBeNull();
  await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
  expect(await within(log).findByText("Added SK hynix")).toBeVisible();
  const rows = within(log).getAllByText(/^(Tracking|Added)/).map(row => row.textContent);
  expect(rows.slice(-2)).toEqual(["Added Samsung Electronics", "Added SK hynix"]);
  expect(screen.getByText("2 on the map · 0 relationships")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Yes, on weekdays" }));
  expect(onFinish).toHaveBeenCalledWith(true);
});

it("says a queued read is waiting its turn", async () => {
  meet(stub({ grows: false, followed: [{ state: "queued", assets: 6, queuedAtMs: Date.now() }] }));
  const log = await screen.findByRole("log");
  const row = await within(log).findByText("Waiting to read what you follow…");
  expect(row.closest(".onboarding-meet-row")?.querySelector(".animate-spin")).not.toBeNull();
  expect(screen.getByText("Names appear here as your Loupist reads what you follow.")).toBeVisible();
});

it("stops saying Reading when a read fails, cannot run yet or takes unusually long", async () => {
  for (const [status, text] of [
    [{ state: "failed", assets: 6 }, "Couldn’t read what you follow. You can start now; the map grows as you research."],
    [{ state: "waiting", assets: 6 }, "Your Loupist reads what you follow once your AI account is ready. You can start now."],
    // Queued four minutes ago and still running.
    [{ state: "running", assets: 6, queuedAtMs: Date.now() - 240_000 }, "Reading what you follow is taking longer than usual. You can start now; the map fills in when it’s done."],
  ] as const) {
    const { unmount } = render(<MeetStep bridge={stub({ grows: false, followed: [status] }) as unknown as LoupistBridge} loupistId="loupist" name="Chip desk" focus="" finishing={false} onFinish={vi.fn()} />);
    const log = await screen.findByRole("log");
    const row = await within(log).findByText(text);
    // A plain line: no spinner anywhere, the empty map says how it grows, and both answers work.
    expect(row.closest(".onboarding-meet-row")?.querySelector(".animate-spin")).toBeNull();
    expect(screen.queryByText("Reading what you follow…")).toBeNull();
    expect(document.querySelector(".onboarding-meet-counter .animate-spin")).toBeNull();
    expect(screen.getByText("Your Loupist's map grows as you research")).toBeVisible();
    expect(screen.getByRole("button", { name: "Yes, on weekdays" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Not now" })).toBeEnabled();
    unmount();
  }
});

it("offers only Start chatting when the user follows nothing", async () => {
  const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
  const onFinish = meet(stub({ watching: false, followed: [{ state: "none", assets: 0 }] }), "");
  expect(await screen.findByRole("heading", { name: "Chip desk is getting ready" })).toBeVisible();
  expect(await screen.findByText("Your Loupist's map grows as you research")).toBeVisible();
  expect(screen.queryByText("Reading what you follow…")).toBeNull();
  expect(screen.getByText("Your Loupist is ready.")).toBeVisible();
  expect(screen.queryByText(/morning update/)).toBeNull();
  expect(screen.queryByRole("button", { name: "Not now" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "Start chatting" }));
  expect(onFinish).toHaveBeenCalledWith(false);
});

it("applies the custom-focus suggestion in one tap", async () => {
  const proposal = { proposalId: "prop-1", kind: "seed" as const, status: "pending" as const, createdAtMs: 0,
    types: [{ name: "Fab", description: "A plant", category: "company" as const, labels: { en: "Fabs", ko: "팹" }, blurbs: { en: "" }, fields: [] }],
    relations: [{ name: "SUPPLIES", description: "", signatures: [], labels: { en: "supplies" } }] };
  const bridge = stub({ proposal, watching: false, followed: [{ state: "none", assets: 0 }] });
  const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
  meet(bridge, "Fabs");
  const card = await screen.findByRole("region", { name: "Your Loupist suggests tracking" });
  expect(within(card).getByText("fabs · supplies")).toBeVisible();
  expect(within(card).getByRole("button", { name: "Keep the default" })).toBeEnabled();
  expect(screen.getByText("You can start now. The map keeps growing while you chat.")).toBeVisible();
  await user.click(within(card).getByRole("button", { name: "Use this" }));
  expect(bridge.worldSchemaApply).toHaveBeenCalledWith({ loupistId: "loupist", proposalId: "prop-1" });
  expect(await screen.findByText("Your Loupist is ready.")).toBeVisible();
});

it("says nothing about reading when World processing is off", async () => {
  meet(stub({ grows: false, followed: [{ state: "off", assets: 1 }] }));
  const log = await screen.findByRole("log");
  await within(log).findByText(/^Tracking:/);
  expect(within(log).queryByText(/what you follow/)).toBeNull();
  expect(screen.getByText("Either choice opens your Loupist.")).toBeVisible();
});
