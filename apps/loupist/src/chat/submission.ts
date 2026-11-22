import i18n from "@/i18n";
import { commandErrorCode, errorMessage, type CommandErrorCode } from "../lib/errors";
import type { FileUIPart } from "ai";
import type { ChatArtifactSelection, ChatKnowledgePin } from "../types";

/** Captured when the user sends or queues; later composer changes cannot alter it. */
export type ChatSubmission = {
  id: string;
  text: string;
  createdAt: string;
  privateChat: boolean;
  modelProfileId: string;
  thinkingLevel: string;
  runOptions: Record<string, string>;
  files: FileUIPart[];
  pins: ChatKnowledgePin[];
  selectedArtifact?: ChatArtifactSelection;
  /** The as-of date this message was sent with; queued messages keep theirs. */
  asOf?: string;
};

export type ChatAdmission =
  | { accepted: true; runId: string }
  | { accepted: false; error: string; code?: CommandErrorCode };

export function admissionRefusal(cause: unknown, fallback: string): Extract<ChatAdmission, { accepted: false }> {
  const code = commandErrorCode(cause);
  const error = code === "memory_busy" ? i18n.t("chat:chatView.waitingForMemory")
    : code === "attachment_too_large" ? i18n.t("chat:chatComposer.attachmentTooLarge") : errorMessage(cause, fallback);
  return { accepted: false, error, ...(code && { code }) };
}
