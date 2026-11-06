import { useState } from "react";
import { useTranslation } from "@/i18n";

export function PrincipleSource({ statement, why, example }: { statement: string; why: string; example: string }) {
  const { t } = useTranslation("knowledge");
  const [open, setOpen] = useState(false);
  const text = [statement, why && `## ${t("memoryPage.reader.why")}\n\n${why}`, example && `## ${t("memoryPage.reader.example")}\n\n${example}`].filter(Boolean).join("\n\n");
  return <details className="lens-basis lens-markdown-source" onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>{t("lensWorkspace.markdownSource")}</summary>
    {open && <pre>{text}</pre>}
  </details>;
}
