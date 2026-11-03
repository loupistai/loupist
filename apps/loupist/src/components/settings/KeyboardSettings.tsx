import i18n, { useTranslation } from "@/i18n";
import { SHORTCUTS, isMacPlatform, shortcutKeys } from "../../app/shortcuts";

/** Settings › Keyboard: every shortcut the app handles, read from the same table as the handlers. Not editable in v1. */
export function KeyboardSettings() {
  useTranslation("settings");
  const mac = isMacPlatform();
  // The page header already says "Keyboard"; the card keeps only its note.
  return <section className="settings-section keyboard-settings" aria-label={i18n.t("settings:keyboard.title")}>
    <div className="settings-section-heading">
      <p>{i18n.t("settings:keyboard.description")}</p>
    </div>
    <table className="keyboard-table">
      <thead><tr><th scope="col">{i18n.t("settings:keyboard.action")}</th><th scope="col">{i18n.t("settings:keyboard.keys")}</th></tr></thead>
      <tbody>
        {SHORTCUTS.map((shortcut) => <tr key={shortcut.id}>
          <td>{i18n.t(`settings:keyboard.actions.${shortcut.id}`)}</td>
          <td>{shortcutKeys(shortcut, mac).split(" · ").map((keys, index) => <span key={keys}>{index ? " · " : null}<kbd>{keys}</kbd></span>)}</td>
        </tr>)}
      </tbody>
    </table>
  </section>;
}
