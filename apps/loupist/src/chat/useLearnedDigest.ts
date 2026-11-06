import { useEffect, useState } from "react";
import type { ChatMessage, LearnedDigest, LoupistBridge } from "../types";
import { showsReceipt } from "./receipt";

export function useLearnedDigest(bridge: LoupistBridge | undefined, loupistId: string, message: ChatMessage, enabled: boolean): LearnedDigest | undefined {
  const [digest, setDigest] = useState<LearnedDigest>();
  const ready = enabled && Boolean(bridge) && showsReceipt(message);
  const commit = message.memory_update?.commitId ?? "";
  useEffect(() => {
    if (!ready || !bridge) { setDigest(undefined); return; }
    let live = true;
    void bridge.memoryLearned({ loupistId, messageIds: [message.id] })
      .then(found => { if (live) setDigest(found.find(item => item.messageId === message.id)); })
      .catch(() => {});
    return () => { live = false; };
  }, [bridge, loupistId, message.id, ready, commit]);
  return digest;
}
