import { createContext, useContext } from "react";
import type { LoupistBridge } from "../../types";

/** Read execution details only within the visible Chat's host-owned scope. */
export type ChatExecutionScope = {
  bridge: LoupistBridge;
  loupistId: string;
  threadId: string;
  surface: "bot" | "session";
  theme: "light" | "dark";
  active: boolean;
};

const ChatExecutionContext = createContext<ChatExecutionScope | null>(null);
export const ChatExecutionProvider = ChatExecutionContext.Provider;
export const useChatExecution = () => useContext(ChatExecutionContext);
