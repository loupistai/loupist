import { createContext, useContext } from "react";

/**
 * Principles and methods the user set aside for the visible chat ("Not this
 * time"). Rust stores the list on the chat and leaves those records out of its
 * context; the renderer only shows the state and submits the change.
 */
export type ChatSetAside = {
  ids: ReadonlySet<string>;
  toggle: (recordId: string, setAside: boolean) => Promise<void>;
  /** The first answer in this chat whose Memory write Rust refused as a secret; the note shows there only. */
  firstDeclinedMessageId?: string;
  /** Saving is off for this chat (private chats), so receipts never list saved items. */
  saveOff?: boolean;
};

/** True when Rust refused one of this message's Memory writes as a credential or secret. */
export const declinedSensitiveMemory = (message: { tool_refusals?: { reason: string }[] }) =>
  Boolean(message.tool_refusals?.some(refusal => refusal.reason === "sensitive_memory"));

const ChatSetAsideContext = createContext<ChatSetAside | null>(null);
export const ChatSetAsideProvider = ChatSetAsideContext.Provider;
export const useChatSetAside = () => useContext(ChatSetAsideContext);
