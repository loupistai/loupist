import i18n from "@/i18n";

const t = i18n.getFixedT(null, "chat");

export function feedbackTeachingPrompt(answer: string, correction: string) {
  const quote = (text: string) => text.split("\n").map((line) => `> ${line}`).join("\n");
  const excerpt = answer.trim().slice(0, 1200);
  return t("answerFeedback.pleaseRevisitThisAnswerUsingMyCorrectionAnswerExcerpt", { value1: quote(excerpt), value2: answer.trim().length > 1200 ? "\n> …" : "", value3: quote(correction) });
}
