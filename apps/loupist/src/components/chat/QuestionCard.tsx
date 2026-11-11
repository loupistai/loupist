import { clockLabel, useNow } from "../../chat/runStatus";
import i18n, { useTranslation } from "@/i18n";
import { useId, useState } from "react";
import type { LoupistBridge } from "../../types";
import type { ChatQuestion } from "../../contracts/chat";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

const t = i18n.getFixedT(null, "chat");

const MAX_TEXT_CHARS = 500;

type Answer = { choice: number } | { text: string };

function WaitingQuestion({ question, onAnswer }: { question: ChatQuestion; onAnswer: (answer: Answer) => Promise<void> }) {
  const nowMs = useNow(true);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [text, setText] = useState("");
  const otherId = useId();
  const answer = async (value: Answer) => {
    setBusy(true);
    setFailed(false);
    try {
      await onAnswer(value);
    } catch {
      setFailed(true);
      setBusy(false);
    }
  };
  const own = text.trim();
  // The rail's "Needs your OK" row scrolls here and moves focus to the card, never to a button.
  return <section className="question-card" data-request-id={question.requestId} tabIndex={-1}
    aria-label={t("questionCard.label", { question: question.question })}>
    <header>
      <strong>{t("questionCard.title")}</strong>
      <span className="question-card-clock">{t("questionCard.waiting", { time: clockLabel(nowMs - question.requestedAtMs) })}</span>
    </header>
    <p className="question-card-text">{question.question}</p>
    <div className="question-card-options" role="group" aria-label={t("questionCard.options")}>
      {question.options.map((option, index) => {
        const recommended = question.recommended === index;
        return <Button key={index} type="button" variant={recommended ? "default" : "outline"} className="question-card-option"
          disabled={busy} onClick={() => void answer({ choice: index })}>
          <span className="question-card-option-label">
            {option.label}
            {recommended ? <span className="question-card-tag">{t("questionCard.recommended")}</span> : null}
          </span>
          {option.description ? <span className="question-card-option-description">{option.description}</span> : null}
        </Button>;
      })}
    </div>
    <form className="question-card-other" onSubmit={(event) => { event.preventDefault(); if (own && !busy) void answer({ text: own }); }}>
      <label htmlFor={otherId} className="question-card-note">{t("questionCard.other")}</label>
      <div className="question-card-other-row">
        <Input id={otherId} value={text} maxLength={MAX_TEXT_CHARS} disabled={busy} autoComplete="off"
          placeholder={t("questionCard.placeholder")} onChange={(event) => setText(event.target.value)} />
        <Button type="submit" variant="outline" disabled={busy || !own}>{t("questionCard.send")}</Button>
      </div>
    </form>
    {failed ? <p role="alert" className="question-card-error">{t("questionCard.error")}</p> : null}
  </section>;
}

function settledLine(question: ChatQuestion): string {
  const answer = question.answer;
  if (question.state === "answered" && answer) {
    return t("questionCard.answered", { answer: "choice" in answer ? question.options[answer.choice]?.label ?? "" : answer.text });
  }
  const assumed = question.recommended === undefined ? undefined : question.options[question.recommended]?.label;
  return assumed ? t("questionCard.unanswered", { option: assumed }) : t("questionCard.unansweredAssumed");
}

/**
 * One question the agent asked mid-reply (`ask_user`): a waiting card with the
 * options, the recommended one first in emphasis, and a free-text answer; once
 * answered or expired it settles into one quiet line. The host resumes the
 * reply on the answer, and a question nobody answers continues with the
 * recommended option.
 */
export function QuestionCard({ question, bridge, loupistId }: { question: ChatQuestion; bridge: LoupistBridge; loupistId: string }) {
  useTranslation("chat");
  if (question.state === "waiting") {
    return <WaitingQuestion question={question}
      onAnswer={(answer) => bridge.chatQuestionAnswer({ loupist_id: loupistId, request_id: question.requestId, ...answer })} />;
  }
  return <p className="question-line" data-state={question.state} data-request-id={question.requestId}>{settledLine(question)}</p>;
}

export function QuestionCards({ questions, bridge, loupistId }: { questions: readonly ChatQuestion[]; bridge: LoupistBridge; loupistId: string }) {
  if (!questions.length) return null;
  return <div className="question-cards">
    {questions.map((question) => <QuestionCard key={question.requestId} question={question} bridge={bridge} loupistId={loupistId} />)}
  </div>;
}
