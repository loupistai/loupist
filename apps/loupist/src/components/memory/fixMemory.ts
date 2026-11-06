import { createContext, useContext } from "react";
import type { KnowledgePin } from "../../types";

export type FixMemory = (pin: KnowledgePin) => void;

const FixMemoryContext = createContext<FixMemory | null>(null);
export const FixMemoryProvider = FixMemoryContext.Provider;
export const useFixMemory = () => useContext(FixMemoryContext);
