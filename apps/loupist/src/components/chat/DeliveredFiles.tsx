import { useState } from "react";
import { EllipsisIcon, FileIcon } from "lucide-react";
import i18n, { useTranslation } from "@/i18n";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";

const t = i18n.getFixedT(null, "chat");

/**
 * Files an answer delivered (side-panel.md X13): one chip per workspace file the
 * answer wrote and linked. The chip opens the file's preview in the panel; its ⋯
 * menu shows the file in Finder through the workspace reveal command.
 */
export function DeliveredFiles({ paths, onOpen, onReveal }: {
  paths: readonly string[];
  onOpen?: (path: string) => void;
  onReveal?: (path: string) => Promise<void>;
}) {
  useTranslation("chat");
  const [failed, setFailed] = useState<string | null>(null);
  if (!paths.length) return null;
  const reveal = (path: string) => {
    setFailed(null);
    void onReveal?.(path).catch(() => setFailed(path));
  };
  return <div className="message-files" role="group" aria-label={t("chatMessageCard.deliveredFiles")}>
    <span className="message-files-label" aria-hidden="true">{t("chatMessageCard.deliveredFiles")}</span>
    {paths.map((path) => {
      const name = path.split("/").at(-1) || path;
      return <span className="message-file-chip" key={path}>
        <button type="button" className="message-file-chip-open" title={path} disabled={!onOpen} onClick={() => onOpen?.(path)}>
          <FileIcon aria-hidden="true" /><span>{name}</span>
        </button>
        {onReveal ? <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button type="button" className="message-file-chip-more" aria-label={t("chatMessageCard.fileActions", { name })}><EllipsisIcon aria-hidden="true" /></button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="message-file-menu">
            <DropdownMenuItem onSelect={() => reveal(path)}>{t("chatMessageCard.showInFinder")}</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu> : null}
      </span>;
    })}
    {failed ? <small className="message-files-error" role="status">{t("chatMessageCard.couldNotShowFile", { name: failed.split("/").at(-1) || failed })}</small> : null}
  </div>;
}
