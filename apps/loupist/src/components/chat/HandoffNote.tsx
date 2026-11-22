import i18n, { useTranslation } from "@/i18n";
import type { ChatMessage, HandoffNote as Note } from "../../types";
import { relationName } from "../map/relations";
import { LoupistAvatar } from "../app/LoupistAvatar";
import type { LoupistAvatarIdentity } from "../../lib/loupistAvatar";
import { SafeMessageResponse } from "./SafeMessageResponse";
import { CopyTextButton } from "../ui/copy-text-button";
import { Disclosure } from "../ui/disclosure";

const t = i18n.getFixedT(null, "chat");

/** The opening is a request authored by Bot and has no answer receipt or editing controls. */
export function BotHandoffOpening({ message, loupist, workspacePath }: {
  message: ChatMessage; loupist: LoupistAvatarIdentity; workspacePath?: string;
}) {
  useTranslation("chat");
  return <article className="message assistant handoff-opening" data-message-id={message.id} data-turn-message="">
    <div className="message-meta"><LoupistAvatar loupist={loupist} size={32} /><span>{t("handoff.fromBot")}</span></div>
    <div className="message-content"><SafeMessageResponse text={message.content} workspacePath={workspacePath} isAnimating={false} /></div>
    {message.handoff_note && <HandoffNote note={message.handoff_note} fromLink={false} />}
    <CopyTextButton label={t("handoff.brief")} text={message.content} />
  </article>;
}

export function HandoffNote({ note, fromLink }: { note: Note; fromLink: boolean }) {
  useTranslation("chat");
  const { t: tk } = useTranslation("knowledge");
  const path = note.linkPath;
  return <Disclosure className="message-handoff-note" title={fromLink ? t("handoff.linkDetails") : t("handoff.details")}>
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
      {note.userMessageInBot && <figure className="handoff-quote">
        <figcaption>{t("handoff.youSaid")}</figcaption>
        <blockquote>{note.userMessageInBot}</blockquote>
      </figure>}
    </>}
  </Disclosure>;
}
