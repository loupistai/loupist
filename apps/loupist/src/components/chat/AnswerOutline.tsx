import i18n, { useTranslation } from "@/i18n";
import { useMemo, useRef, useState } from "react";
import { ListIcon } from "lucide-react";
import { answerOutline } from "../../chat/outline";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

const t = i18n.getFixedT(null, "chat");

/**
 * "Outline" for a long answer (chat spec X16; chat design change 18): a small trigger at
 * the end of the answer's meta row, so it is on the first line, opening its H2/H3
 * headings as links that scroll to and focus the heading in the rendered answer.
 */
export function AnswerOutline({ messageId, markdown }: { messageId: string; markdown: string }) {
  useTranslation("chat");
  const [open, setOpen] = useState(false);
  const jumped = useRef(false);
  const rows = useMemo(() => answerOutline(markdown), [markdown]);
  if (!rows.length) return null;
  const jump = (index: number) => {
    const { text } = rows[index]!;
    const article = [...document.querySelectorAll("[data-message-id]")].find((element) => element.getAttribute("data-message-id") === messageId);
    // The answer's prose may be split around cards and follow a narration preamble; the
    // second heading of the same name is the second one rendered with that name.
    const occurrence = rows.slice(0, index).filter((row) => row.text === text).length;
    const headings = [...(article?.querySelectorAll<HTMLElement>(".message-content:not(.message-narration) :is(h2, h3)") ?? [])];
    const heading = headings.filter((element) => element.textContent?.trim() === text)[occurrence];
    jumped.current = Boolean(heading);
    setOpen(false);
    if (!heading) return;
    heading.setAttribute("tabindex", "-1");
    heading.scrollIntoView({ block: "start", behavior: "smooth" });
    heading.focus({ preventScroll: true });
  };
  return <Popover open={open} onOpenChange={setOpen}>
    <PopoverTrigger asChild>
      <button type="button" className="answer-outline-trigger" data-answer-outline><ListIcon aria-hidden="true" />{t("chatOutline.title")}</button>
    </PopoverTrigger>
    {/* Choosing a heading moves focus there; Escape still returns focus to the trigger. */}
    <PopoverContent className="answer-outline" align="end" aria-label={t("chatOutline.title")} onCloseAutoFocus={(event) => { if (jumped.current) event.preventDefault(); jumped.current = false; }}>
      <ol>
        {rows.map((row, index) => <li key={`${index}:${row.text}`} data-level={row.level}>
          <button type="button" onClick={() => jump(index)}>{row.text}</button>
        </li>)}
      </ol>
    </PopoverContent>
  </Popover>;
}
