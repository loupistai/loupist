import i18n, { useTranslation } from "@/i18n";
import type { HandoffNote as Note } from "../../types";
import { relationName } from "../map/relations";
import { Disclosure } from "../ui/disclosure";

const t = i18n.getFixedT(null, "chat");

/**
 * What came with a hand-off session's first message: the Bot's note, or the link a chosen
 * action named. Closed by default and never a bubble of its own, so the user's request reads
 * first and the note is never taken for something the user wrote. Text renders as text.
 */
export function HandoffNote({ note, fromLink }: { note: Note; fromLink: boolean }) {
  useTranslation("chat");
  const { t: tk } = useTranslation("knowledge");
  const path = note.linkPath;
  return <Disclosure className="message-handoff-note" title={fromLink ? t("handoff.linkDetails") : t("handoff.fromBot")}>
    {fromLink ? path ? <>
      <p>{t("handoff.linkStart", { name: path.seed })}</p>
      <ol>
        {path.path.map(({ from, relation, to }, index) => <li key={index}>
          {t("handoff.linkClaim", { from, relation: relationName(relation, tk), to })}
        </li>)}
      </ol>
      <p>{t("handoff.linkEnd", { name: path.far })}</p>
    </> : <p>{t("handoff.linkUnavailable")}</p> : <>
      <p className="message-handoff-note-title">{note.title}</p>
      {note.question ? <p>{note.question}</p> : null}
      {note.brief ? <p>{note.brief}</p> : null}
    </>}
  </Disclosure>;
}
