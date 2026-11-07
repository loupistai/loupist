import type { ChatMessage } from "../types";

/** A setup card an answer shows inline, keyed by the catalog entry the host resolved. */
export type ConnectionSetupBlock = { key: string; entryId: string };

/**
 * Cards for the connections an answer offered to set up: one per completed
 * `connection_setup` step, whose target is the resolved catalog entry id. A
 * failed step (no such connection, a non-interactive surface) draws nothing,
 * and one entry asked for twice gets one card.
 */
export function connectionSetupBlocks(message: ChatMessage): ConnectionSetupBlock[] {
  if (message.role !== "assistant") return [];
  const seen = new Set<string>();
  const blocks: ConnectionSetupBlock[] = [];
  for (const item of message.progress?.items ?? []) {
    if (item.kind !== "tool" || item.toolName !== "connection_setup" || item.status !== "succeeded") continue;
    const entryId = item.target?.trim();
    if (!entryId || seen.has(entryId)) continue;
    seen.add(entryId);
    blocks.push({ key: `connection-setup:${entryId}`, entryId });
  }
  return blocks;
}
