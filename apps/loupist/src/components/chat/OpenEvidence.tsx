import { createContext, useContext, type ReactNode } from "react";
import { shownEvidenceId, type ChatWorkspaceSession } from "../../chat/workspace";
import { useChatMap } from "../workspace/ChatMapContext";

/** The evidence whose passage the side panel's Evidence reader shows now, if any. */
const OpenEvidenceContext = createContext<string | undefined>(undefined);

export const useOpenEvidenceId = () => useContext(OpenEvidenceContext);

/** Marks the citations of the passage the open panel shows (the chat map, when shown, hides Evidence). */
export function OpenEvidenceProvider({ session, open, children }: { session: ChatWorkspaceSession; open: boolean; children: ReactNode }) {
  const chatMap = useChatMap();
  const id = open && !chatMap?.active ? shownEvidenceId(session) : undefined;
  return <OpenEvidenceContext.Provider value={id}>{children}</OpenEvidenceContext.Provider>;
}
