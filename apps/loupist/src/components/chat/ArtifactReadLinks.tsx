import { createContext, useContext, useMemo, type ReactNode } from "react";
import type { ChatMessage } from "../../types";
import { artifactReadTarget } from "../../chat/artifactReadTarget";

const ArtifactReadLinks = createContext<ReadonlyMap<string, string>>(new Map());
export const useArtifactReadLinks = () => useContext(ArtifactReadLinks);

/** A later answer can link work read earlier in this chat. Other citations stay turn-scoped. */
export function ArtifactReadLinksProvider({ messages, children }: { messages: readonly ChatMessage[]; children: ReactNode }) {
  const links = useMemo(() => {
    const retained = new Map<string, string>();
    for (const message of messages) for (const evidence of message.evidence_refs ?? []) {
      const artifactId = artifactReadTarget(evidence);
      if (artifactId) retained.set(`#evidence-${evidence.id}`, artifactId);
    }
    return retained;
  }, [messages]);
  return <ArtifactReadLinks.Provider value={links}>{children}</ArtifactReadLinks.Provider>;
}
