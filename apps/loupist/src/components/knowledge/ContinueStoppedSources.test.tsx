import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { GraphProcessingView, SourcesRetried } from "../../types";
import { ContinueStoppedSources } from "./SourceRetry";

function bridge(maxLlmCalls = 32) {
  let allowance = maxLlmCalls;
  return {
    knowledgeGraphSettingsGet: vi.fn(async () => ({ settings: { maxLlmCalls: allowance } }) as unknown as GraphProcessingView),
    knowledgeGraphRetrySources: vi.fn<(request: { loupistId: string; recordIds: string[]; maxLlmCalls: number }) => Promise<SourcesRetried>>(),
    setAllowance(next: number) { allowance = next; },
  };
}

it("asks once with the count and the most requests, and sends exactly the Sources it counted", async () => {
  const user = userEvent.setup();
  const host = bridge(32);
  const continued = vi.fn();
  let resolve: (value: SourcesRetried) => void = () => {};
  host.knowledgeGraphRetrySources.mockImplementation(() => new Promise(done => { resolve = done; }));
  const view = render(<ContinueStoppedSources bridge={host} loupistId="loupist-a" recordIds={["s1", "s2", "s3"]} onContinued={continued} />);
  await user.click(screen.getByRole("button", { name: "Continue all" }));
  const dialog = await screen.findByRole("dialog", { name: "Continue reading 3 Sources?" });
  expect(dialog).toHaveTextContent("This can use up to 96 model requests.");
  // A Source stopped after the dialog opened is not among those it asked about.
  view.rerender(<ContinueStoppedSources bridge={host} loupistId="loupist-a" recordIds={["s1", "s2", "s3", "s4"]} onContinued={continued} />);
  const run = within(dialog).getByRole("button", { name: "Continue" });
  await user.click(run);
  // A second click while the host decides sends nothing more.
  expect(within(dialog).getByRole("button", { name: "Continuing…" })).toBeDisabled();
  expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeDisabled();
  expect(host.knowledgeGraphRetrySources).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-a", recordIds: ["s1", "s2", "s3"], maxLlmCalls: 32 });
  await act(async () => resolve({ continued: 2, skipped: 1 }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(continued).toHaveBeenCalledOnce();
});

it("says when none of them can continue and keeps the host's refusal in view", async () => {
  const user = userEvent.setup();
  const host = bridge(8);
  host.knowledgeGraphRetrySources.mockResolvedValueOnce({ continued: 0, skipped: 1 });
  host.knowledgeGraphRetrySources.mockRejectedValueOnce(new Error("the request allowance changed since this was confirmed"));
  const continued = vi.fn();
  render(<ContinueStoppedSources bridge={host} loupistId="loupist-a" recordIds={["s1"]} onContinued={continued} />);
  await user.click(screen.getByRole("button", { name: "Continue all" }));
  const dialog = await screen.findByRole("dialog", { name: "Continue reading 1 Source?" });
  expect(dialog).toHaveTextContent("This can use up to 8 model requests.");
  await user.click(within(dialog).getByRole("button", { name: "Continue" }));
  expect(await within(dialog).findByRole("alert")).toHaveTextContent("None of these Sources can continue from here.");
  await user.click(within(dialog).getByRole("button", { name: "Continue" }));
  expect(await within(dialog).findByRole("alert")).toHaveTextContent("the request allowance changed since this was confirmed");
  expect(continued).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  // Opening again reads the allowance saved now.
  host.setAllowance(16);
  await user.click(screen.getByRole("button", { name: "Continue all" }));
  expect(await screen.findByRole("dialog", { name: "Continue reading 1 Source?" })).toHaveTextContent("This can use up to 16 model requests.");
});
