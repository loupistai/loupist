import { createContext, useContext } from "react";
import type { KnowledgePin } from "../../types";

/** Opens the Bot composer with a page pinned and "Correct this:" ahead of the cursor; it never sends. The app shell provides it. */
export type FixMemory = (pin: KnowledgePin) => void;

const FixMemoryContext = createContext<FixMemory | null>(null);
export const FixMemoryProvider = FixMemoryContext.Provider;
/** Null outside the shell: Fix hides itself. */
export const useFixMemory = () => useContext(FixMemoryContext);
