import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ClaimHistory } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { dateValue } from "../../knowledge/presentation";
import { historyEpisode } from "./claimHistory";
import { ClaimHistoryPanel } from "./MapClaimInspector";

const episode = (title: string, recordId: string, publicationTime: string) => ({
  sourceTitle: title,
  publicationTime,
  readArgs: { scope: "source", recordId, revisionId: `${recordId}-rev`, spanId: `${recordId}-span` },
});

it("keeps a source title, a date and a passage from an episode", () => {
  expect(historyEpisode({
    sourceTitle: "Later filing",
    publicationTime: "2025-01-01",
    end: { precision: "date", value: "2025-06-01" },
    readArgs: { recordId: "rec-2", spanId: "span-2" },
  })).toMatchObject({ title: "Later filing", date: "2025-06-01", recordId: "rec-2", spanId: "span-2" });
});

it("lists who created, supported and ended a claim without printing claim ids", async () => {
  const history: ClaimHistory = {
    created: episode("First filing", "rec-created", "2024-03-31"),
    supported: Array.from({ length: 6 }, (_, index) => episode(`Support ${index + 1}`, `rec-s-${index}`, "2024-06-01")),
    ended: { ...episode("Closing note", "rec-ended", "2025-01-01"), end: { precision: "date", value: "2025-06-01" } },
    endedClaims: ["claim-quiet"],
    endedByClaimId: "claim-ender",
    endings: [],
  };
  const knowledgeWorldClaimHistory = vi.fn(async () => history);
  const onOpen = vi.fn();
  const user = userEvent.setup();
  render(<ClaimHistoryPanel bridge={{ knowledgeWorldClaimHistory } as unknown as LoupistBridge} location={{ loupistId: "loupist", surface: "world" }} claimId="claim-1" onOpen={onOpen} />);
  expect(knowledgeWorldClaimHistory).not.toHaveBeenCalled();
  await user.click(screen.getByText("History"));
  expect(await screen.findByRole("button", { name: "First filing" })).toBeVisible();
  expect(screen.getByText(new RegExp(dateValue("2024-03-31")))).toBeTruthy();
  expect(screen.getByRole("button", { name: "Support 5" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Support 6" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "+1" }));
  expect(screen.getByRole("button", { name: "Support 6" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Closing note" })).toBeVisible();
  expect(screen.queryByText("claim-ender")).toBeNull();
  expect(screen.queryByText("claim-quiet")).toBeNull();
  await user.click(screen.getByRole("button", { name: "Open the claim that ended it" }));
  expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ surface: "world", selection: { kind: "claim", id: "claim-ender" } }));
  await user.click(screen.getByRole("button", { name: "Open claim" }));
  expect(onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ selection: { kind: "claim", id: "claim-quiet" } }));
  await user.click(screen.getByRole("button", { name: "First filing" }));
  expect(onOpen).toHaveBeenLastCalledWith(expect.objectContaining({
    surface: "source", recordId: "rec-created", revisionId: "rec-created-rev",
    selection: expect.objectContaining({ kind: "source", spanId: "rec-created-span" }),
  }));
});
