import i18n, { useTranslation } from "@/i18n";
import { formatDate } from "@/i18n/format";
import { useState } from "react";
import { ImageDownIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { exportFileName } from "../../chat/exportChat";
import { chartPngBytes, type ChartSnapshotSlot } from "./chartSnapshot";

const t = i18n.getFixedT(null, "chat");

export type SavePng = (request: { suggestedName: string; kind: "png"; bytes: number[] }) => Promise<string | null>;

/**
 * "Save as PNG…" for a drawn chart (shell-briefings §2.6, X9): a 2× picture of
 * the chart with the export footer, saved through the native dialog (Rust
 * `export_save`). Cancelling the dialog is not an error.
 */
/** The save itself, for a button or a menu item. */
export function useSaveChartPng(slot: ChartSnapshotSlot, title: string, onSave: SavePng) {
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const save = async () => {
    const snapshot = slot.current;
    if (busy) return;
    if (!snapshot) { setFailed(true); return; }
    setBusy(true);
    setFailed(false);
    try {
      const bytes = await chartPngBytes(snapshot, { title, date: formatDate(Date.now()), disclosure: i18n.t("knowledge:map.export.disclosure") });
      await onSave({ suggestedName: `${exportFileName(title || t("chatBlock.untitled"))}.png`, kind: "png", bytes });
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };
  return { save, busy, failed };
}

export function SaveChartPng({ slot, title, onSave, textOnly = false }: { slot: ChartSnapshotSlot; title: string; onSave: SavePng; textOnly?: boolean }) {
  useTranslation("chat");
  const { save, busy, failed } = useSaveChartPng(slot, title, onSave);
  return <>
    {failed ? <span className="chat-artifact-block-error" role="alert">{t("chartExport.couldNotSave")}</span> : null}
    <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void save()}>
      {textOnly ? null : <ImageDownIcon aria-hidden="true" />}{t("chartExport.savePng")}
    </Button>
  </>;
}
