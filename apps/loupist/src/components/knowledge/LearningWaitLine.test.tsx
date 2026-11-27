import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { LearningState, LearningWaitReason } from "@/contracts/knowledge";
import type { LoupistBridge } from "@/types";
import { LearningWaitLine } from "./LearningWaitLine";

const bridgeWith = (read: () => Promise<LearningState>) => ({ knowledgeLearningState: vi.fn(read) }) as unknown as LoupistBridge;
const waiting = (reason: LearningWaitReason): LearningState => ({ state: "waiting", reason, cases: 1 });

it.each<[LearningWaitReason, RegExp]>([
  [{ kind: "off" }, /^Method learning is off for this Loupist\.$/],
  [{ kind: "no_second_case" }, /^A method is learned once a second, separate case shows it\.$/],
  [{ kind: "subjects_pending", n: 1 }, /^Waiting for 1 subject to be confirmed in World\.$/],
  [{ kind: "subjects_pending", n: 3 }, /^Waiting for 3 subjects to be confirmed in World\.$/],
  [{ kind: "sources_unread", n: 12 }, /^Waiting for 12 Sources to be read into World\.$/],
  [{ kind: "rate_limited", untilMs: Date.UTC(2026, 10, 27, 9, 30) }, /^Waiting for the AI account's limit, until Nov 27, .+\.$/],
  [{ kind: "allowance" }, /^Waiting for tomorrow's learning allowance\.$/],
])("says why method learning waits: %o", async (reason, line) => {
  render(<LearningWaitLine bridge={bridgeWith(async () => waiting(reason))} loupistId="loupist-c" />);
  expect(await screen.findByRole("status")).toHaveTextContent(line);
});

it("says nothing while method learning is not waiting, and offers Try again when the read fails", async () => {
  let calls = 0;
  const bridge = bridgeWith(async () => { calls += 1; if (calls === 1) throw new Error("offline"); return { state: "running", reason: null, cases: 2 }; });
  const { container } = render(<LearningWaitLine bridge={bridge} loupistId="loupist-c" />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't read why method learning is waiting.");
  await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }));
  await vi.waitFor(() => expect(container).toBeEmptyDOMElement());
  expect(bridge.knowledgeLearningState).toHaveBeenCalledWith({ loupistId: "loupist-c" });
});
