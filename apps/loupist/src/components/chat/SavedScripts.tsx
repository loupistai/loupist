import { useState } from "react";
import { useTranslation } from "../../i18n";
import { errorMessage } from "../../lib/errors";
import type { ScriptRef } from "../../contracts/generated";
import type { LoupistBridge } from "../../types";
import { Button } from "../ui/button";

export function SavedScripts({ scripts, bridge, loupistId }: {
  scripts: ScriptRef[];
  bridge?: LoupistBridge;
  loupistId: string;
}) {
  const { t } = useTranslation("chat");
  const [removed, setRemoved] = useState<string[]>([]);
  const [pending, setPending] = useState<string>();
  const [error, setError] = useState<string>();
  const undo = async (script: ScriptRef, key: string) => {
    if (!bridge) return;
    setPending(key);
    setError(undefined);
    try {
      await bridge.scriptDelete({ loupistId, ...script });
      setRemoved(current => [...current, key]);
    } catch (cause) { setError(errorMessage(cause, t("python.undoError"))); }
    finally { setPending(undefined); }
  };
  const remaining = scripts.filter(script => !removed.includes(`${script.name}@${script.version}`));
  if (!remaining.length) return null;
  return <section aria-label={t("python.savedScripts")} className="learned-page">
    <h4>{t("python.savedScripts")}</h4>
    {error ? <p role="alert">{error}</p> : null}
    <ul>{remaining.map(script => {
      const key = `${script.name}@${script.version}`;
      return <li key={key}><code>{key}</code>{bridge ? <Button size="sm" variant="ghost" disabled={pending !== undefined} onClick={() => void undo(script, key)}>{t("learnedCard.undo")}</Button> : null}</li>;
    })}</ul>
  </section>;
}
