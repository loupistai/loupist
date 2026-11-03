import i18n from "@/i18n";
import type { LoupistBridge, LensProposal } from "../types";

const t = i18n.getFixedT(null, "chat");

export const matchLensProposal = (
  proposals: readonly LensProposal[],
  target: { recordId: string; chatId?: string; messageId?: string },
): LensProposal | undefined => {
  if (!target.chatId || !target.messageId) return undefined;
  const matches = proposals.filter((item) => item.recordId === target.recordId
    && item.originChatId === target.chatId && item.originMessageId === target.messageId);
  return matches.length === 1 ? matches[0] : undefined;
};

export const proposalMatchError = () => t("dismissLensProposal.couldNotIdentifyExactlyOneProposalForThisAnswer");

export const dismissLensProposal = async (
  bridge: Pick<LoupistBridge, "knowledgeLensProposals" | "knowledgeLensProposalDismiss">,
  target: { loupistId: string; recordId: string; chatId?: string; messageId?: string },
) => {
  const proposals = await bridge.knowledgeLensProposals({ loupistId: target.loupistId, recordId: target.recordId });
  const match = matchLensProposal(proposals, target);
  if (!match) throw new Error(proposalMatchError());
  await bridge.knowledgeLensProposalDismiss({ loupistId: target.loupistId, draftId: match.draftId });
};
