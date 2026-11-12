import { readKnowledgeMap } from "./mapRead";
import type { LoupistBridge } from "../types";

it("shares one Map read while it is in flight, and asks again after it settles or when fresh", async () => {
  const knowledgeMap = vi.fn(async () => ({ nodes: [], edges: [] }));
  const bridge = { knowledgeMap } as unknown as LoupistBridge;
  const first = readKnowledgeMap(bridge, "a");
  expect(readKnowledgeMap(bridge, "a")).toBe(first);
  expect(readKnowledgeMap(bridge, "b")).not.toBe(first);
  const fresh = readKnowledgeMap(bridge, "a", { fresh: true });
  expect(fresh).not.toBe(first);
  expect(readKnowledgeMap(bridge, "a")).toBe(fresh);
  await fresh;
  await first;
  readKnowledgeMap(bridge, "a");
  expect(knowledgeMap).toHaveBeenCalledTimes(4);
  expect(knowledgeMap).toHaveBeenCalledWith({ loupistId: "a", maxNodes: 1000 });
});
