import { useEffect, useState } from "react";
import { useTranslation } from "@/i18n";
import type { LoupistBridge } from "../../types";
import type { PackOrigin } from "../../contracts/sharing";
import { Button } from "../ui/button";
import "@/styles/sharing.css";

/**
 * "Shared · {pack} by {author}" under a Lens or Map title (W19.4). Nothing is
 * drawn when the record did not come from a shared Loupist.
 */
export function SharedOrigin({ bridge, loupistId, subjectId, onAbout }: { bridge: LoupistBridge; loupistId: string; subjectId?: string | null; onAbout?: () => void }) {
  const { t } = useTranslation("sharing");
  const [origin, setOrigin] = useState<PackOrigin | null>(null);
  useEffect(() => {
    setOrigin(null);
    if (!subjectId) return;
    let live = true;
    void bridge.packOrigin({ loupistId, subjectId }).then(value => { if (live) setOrigin(value); }).catch(() => undefined);
    return () => { live = false; };
  }, [bridge, loupistId, subjectId]);
  if (!origin) return null;
  return <p className="shared-origin" data-shared-origin>
    <span>{origin.author ? t("sharedFrom", { pack: origin.packName, author: origin.author }) : `${t("sharedTag")} · ${origin.packName}`}</span>
    {onAbout ? <Button variant="link" size="sm" onClick={onAbout}>{t("aboutPack")}</Button> : null}
  </p>;
}
