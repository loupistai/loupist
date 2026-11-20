import type { CSSProperties } from "react";
import { LOUPIST_PORTRAIT_IDS, type LoupistAvatarId } from "../../contracts/loupist";
import { LOUPIST_FACE, loupistFaceToken, loupistPortraitId, type LoupistAvatarIdentity } from "../../lib/loupistAvatar";
import { useTranslation } from "../../i18n";
import "@/styles/loupist-face.css";

/**
 * The Loupist's face: a round character holding a loupe, in its face colour. It fills
 * its box, so callers size it in CSS (as the shell does) or with `size` (24 to 64 px).
 * `busy` turns a thin ring around it and moves its eyes while it works.
 */
export function LoupistAvatar({ loupist, size, busy = false, className }: {
  loupist: LoupistAvatarIdentity; size?: number; busy?: boolean; className?: string;
}) {
  const id = loupistPortraitId(loupist);
  const style = { "--loupist-face-color": `var(${loupistFaceToken(id)})`, ...(size ? { width: size, height: size, flexBasis: size } : {}) } as CSSProperties;
  const { lens, eye } = LOUPIST_FACE;
  return <span className={className ? `loupist-avatar loupist-face ${className}` : "loupist-avatar loupist-face"} data-avatar={id} data-busy={busy || undefined} style={style} aria-hidden="true">
    <svg viewBox="0 0 48 48" focusable="false">
      <path className="loupist-face-body" d={LOUPIST_FACE.body} />
      <circle className="loupist-face-lens" cx={lens.cx} cy={lens.cy} r={lens.r} />
      <path className="loupist-face-handle" d={LOUPIST_FACE.handle} />
      <g className="loupist-face-eyes">{LOUPIST_FACE.eyes.map(({ cx, cy }) => <ellipse key={cx} cx={cx} cy={cy} rx={eye.rx} ry={eye.ry} />)}</g>
      <path className="loupist-face-mouth" d={LOUPIST_FACE.mouth} />
    </svg>
  </span>;
}

/** Ten face colours, one per stored portrait id. No new stored values. */
export function LoupistAvatarPicker({ loupist, onChange, disabled = false }: { loupist: LoupistAvatarIdentity; onChange: (avatar: LoupistAvatarId) => void; disabled?: boolean }) {
  const { t } = useTranslation("app");
  const selected = loupistPortraitId(loupist);
  return <fieldset className="loupist-profile-avatars" disabled={disabled}><legend>{t("loupistProfile.avatar")}</legend><div>
    {LOUPIST_PORTRAIT_IDS.map(avatar => <button type="button" key={avatar} aria-label={t(`loupistProfile.faces.${avatar}`)} title={t(`loupistProfile.faces.${avatar}`)} aria-pressed={selected === avatar} onClick={() => onChange(avatar)}><LoupistAvatar loupist={{ ...loupist, avatar }} /></button>)}
  </div></fieldset>;
}
