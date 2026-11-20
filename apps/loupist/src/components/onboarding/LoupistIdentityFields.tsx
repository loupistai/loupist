import { useTranslation } from "@/i18n";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { LoupistAvatarId } from "@/contracts/loupist";
import { LoupistAvatar, LoupistAvatarPicker } from "../app/LoupistAvatar";
import "@/styles/onboarding.css";

/**
 * Who the Loupist is: its face at full size with the name beside it, the Name field and
 * the ten face colours. Onboarding's name step, New Loupist and Loupist settings share it,
 * so a name is always typed by the user (a template only suggests one).
 */
export function LoupistIdentityFields({ id, name, avatar, focus, onChange, busy = false, idPrefix, invalid = false }: {
  id?: string; name: string; avatar: LoupistAvatarId; focus?: string;
  onChange: (identity: { name: string; avatar: LoupistAvatarId }) => void;
  busy?: boolean; idPrefix: string; invalid?: boolean;
}) {
  const { t } = useTranslation("onboarding");
  const trimmed = name.trim();
  return <div className="loupist-identity" data-loupist-identity>
    <div className="loupist-identity-preview" aria-hidden="true">
      <LoupistAvatar loupist={{ id, name, avatar }} size={64} />
      <span><strong data-empty={!trimmed || undefined}>{trimmed || t("wizard.nameTitle")}</strong>{focus?.trim() ? <small>{focus.trim()}</small> : null}</span>
    </div>
    <div className="form-field"><Label htmlFor={`${idPrefix}-name`}>{t("wizard.name")}</Label>
      <Input id={`${idPrefix}-name`} value={name} maxLength={60} disabled={busy} aria-invalid={invalid || undefined}
        onChange={event => onChange({ name: event.target.value, avatar })} /></div>
    <LoupistAvatarPicker loupist={{ id, name, avatar }} disabled={busy} onChange={next => onChange({ name, avatar: next })} />
  </div>;
}
