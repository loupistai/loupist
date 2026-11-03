import i18n, { useTranslation } from "@/i18n";
import { useMemo } from "react";
import { answerOutline } from "../../chat/outline";

const t = i18n.getFixedT(null, "chat");

/**
 * "Outline" above a long answer (chat spec X16): its H2/H3 headings as links
 * that jump to the heading in the rendered answer. Collapsed by default;
 * briefings pass `defaultOpen`.
 */
export function AnswerOutline({ messageId, markdown, defaultOpen = false }: { messageId: string; markdown: string; defaultOpen?: boolean }) {
  useTranslation("chat");
  const rows = useMemo(() => answerOutline(markdown), [markdown]);
  if (!rows.length) return null;
  const jump = (text: string) => {
    const article = [...document.querySelectorAll("[data-message-id]")].find((element) => element.getAttribute("data-message-id") === messageId);
    const answer = article?.querySelector(".message-content");
    const heading = [...(answer?.querySelectorAll<HTMLElement>("h2, h3") ?? [])].find((element) => element.textContent?.trim() === text);
    if (!heading) return;
    heading.setAttribute("tabindex", "-1");
    heading.scrollIntoView({ block: "start", behavior: "smooth" });
    heading.focus({ preventScroll: true });
  };
  return <details className="answer-outline" open={defaultOpen || undefined}>
    <summary>{t("chatOutline.title")}</summary>
    <ol>
      {rows.map((row, index) => <li key={`${index}:${row.text}`} data-level={row.level}>
        <button type="button" onClick={() => jump(row.text)}>{row.text}</button>
      </li>)}
    </ol>
  </details>;
}
