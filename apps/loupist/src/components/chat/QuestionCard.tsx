import { clockLabel, useNow } from "../../chat/runStatus";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useId, useRef, useState } from "react";
import type { LoupistBridge } from "../../types";
import type { ChatQuestion } from "../../contracts/chat";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

const t = i18n.getFixedT(null, "chat");

const MAX_TEXT_CHARS = 500;

type Answer = { choice: number } | { text: string };

/** Typing elsewhere is never interrupted by a card arriving. */
const editingElsewhere = () => {
  const active = document.activeElement;
  return active instanceof HTMLElement && (active.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(active.tagName));
};

/**
 * Answering collapses the card to one line. The turn keeps the height it has
 * now while the reply runs (`--turn-hold`, chat.css), so the scroll range never
 * shrinks under the reader: the line settles in place and the reply continues
 * below it. Returns the release, for an answer that did not go through.
 */
export function holdTurnHeight(node: HTMLElement | null): () => void {
  const turn = node?.closest<HTMLElement>(".turn");
  if (!turn) return () => {};
  const height = Math.ceil(turn.getBoundingClientRect().height);
  turn.style.setProperty("--turn-hold", `${height}px`);
  // A glide may have grown the turn with an inline minimum (conversation.tsx `reserve`), which outranks chat.css.
  const inline = turn.style.minHeight;
  if (inline && Number.parseFloat(inline) < height) turn.style.minHeight = `${height}px`;
  return () => {
    turn.style.removeProperty("--turn-hold");
    if (inline) turn.style.minHeight = inline;
  };
}

function WaitingQuestion({ question, onAnswer, focusOnArrive }: { question: ChatQuestion; onAnswer: (answer: Answer) => Promise<void>; focusOnArrive: boolean }) {
  const nowMs = useNow(true);
  const recommendedRef = useRef<HTMLButtonElement>(null);
  const cardRef = useRef<HTMLElement>(null);
  // The recommended answer takes focus only when the card arrives after the reply stopped streaming,
  // so Enter chooses it; while text is still streaming, focus stays where the reader left it.
  useEffect(() => {
    if (focusOnArrive && !editingElsewhere()) recommendedRef.current?.focus({ preventScroll: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only the card's arrival moves focus.
  }, []);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [text, setText] = useState("");
  const otherId = useId();
  const answer = async (value: Answer) => {
    setBusy(true);
    setFailed(false);
    const release = holdTurnHeight(cardRef.current);
    try {
      await onAnswer(value);
    } catch {
      release();
      setFailed(true);
      setBusy(false);
    }
  };
  const own = text.trim();
  // The rail's "Needs your OK" row scrolls here and moves focus to the card, never to a button.
  return <section ref={cardRef} className="question-card" data-request-id={question.requestId} tabIndex={-1}
    aria-label={t("questionCard.label", { question: question.question })}>
    <header>
      <strong>{t("questionCard.title")}</strong>
      <span className="question-card-clock">{t("questionCard.waiting", { time: clockLabel(nowMs - question.requestedAtMs) })}</span>
    </header>
    <p className="question-card-text">{question.question}</p>
    <div className="question-card-options" role="group" aria-label={t("questionCard.options")}>
      {question.options.map((option, index) => {
        const recommended = question.recommended === index;
        return <Button key={index} ref={recommended ? recommendedRef : undefined} type="button" variant="outline" className="question-card-option" data-recommended={recommended || undefined}
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
export function QuestionCard({ question, bridge, loupistId, focusOnArrive = false }: { question: ChatQuestion; bridge: LoupistBridge; loupistId: string; focusOnArrive?: boolean }) {
  useTranslation("chat");
  if (question.state === "waiting") {
    return <WaitingQuestion question={question} focusOnArrive={focusOnArrive}
      onAnswer={(answer) => bridge.chatQuestionAnswer({ loupist_id: loupistId, request_id: question.requestId, ...answer })} />;
  }
  return <p className="question-line" data-state={question.state} data-request-id={question.requestId}>{settledLine(question)}</p>;
}

export function QuestionCards({ questions, bridge, loupistId, streaming = false }: { questions: readonly ChatQuestion[]; bridge: LoupistBridge; loupistId: string; streaming?: boolean }) {
  if (!questions.length) return null;
  return <div className="question-cards">
    {questions.map((question, index) => <QuestionCard key={question.requestId} question={question} bridge={bridge} loupistId={loupistId} focusOnArrive={!streaming && index === 0} />)}
  </div>;
}
