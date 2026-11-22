import type { ChatMessage } from "../types";

/** Independent posts can arrive after a reply without becoming part of its turn. */
const isBotPost = (message: ChatMessage) =>
  Boolean(message.delivery || message.alert || message.vocabulary || message.handoff || message.link)
  || (message.host_composed === true && !message.execution_model)
  || message.id.startsWith("price-setup-reply-");

export function lastTurnMessageIndex(messages: readonly ChatMessage[], before = messages.length): number {
  for (let index = before - 1; index >= 0; index -= 1) {
    const message = messages[index]!;
    if (!message.steer && !isBotPost(message)) return index;
  }
  return -1;
}
