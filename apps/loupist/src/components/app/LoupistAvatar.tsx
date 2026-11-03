import { LOUPIST_PORTRAIT_IDS, type LoupistAvatarId } from "../../contracts/loupist";
import { loupistAvatarSource, loupistPortraitId, type LoupistAvatarIdentity } from "../../lib/loupistAvatar";
import { useTranslation } from "../../i18n";

export function LoupistAvatar({ loupist }: { loupist: LoupistAvatarIdentity }) {
  return <span className="loupist-avatar" data-avatar={loupistPortraitId(loupist)} aria-hidden="true"><img src={loupistAvatarSource(loupist)} alt="" draggable={false} /></span>;
}

export function LoupistAvatarPicker({ loupist, onChange, disabled = false }: { loupist: LoupistAvatarIdentity; onChange: (avatar: LoupistAvatarId) => void; disabled?: boolean }) {
  const { t } = useTranslation("app");
  const selected = loupistPortraitId(loupist);
  return <fieldset className="loupist-profile-avatars" disabled={disabled}><legend>{t("loupistProfile.avatar")}</legend><div>
    {LOUPIST_PORTRAIT_IDS.map(avatar => <button type="button" key={avatar} aria-label={t(`loupistProfile.avatars.${avatar}`)} title={t(`loupistProfile.avatars.${avatar}`)} aria-pressed={selected === avatar} onClick={() => onChange(avatar)}><LoupistAvatar loupist={{ ...loupist, avatar }} /></button>)}
  </div></fieldset>;
}
