import i18n, { useTranslation } from "@/i18n";
import { MoreHorizontalIcon } from "lucide-react";
import { Button } from "../ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "../ui/dropdown-menu";

/**
 * A followed asset's row menu on the Following page: open its asset page, or stop
 * following it (the same `watchlist_remove` every Loupist shares; no confirmation).
 */
export function FollowedMenu({ name, disabled, onOpen, onStop }: {
  name: string; disabled?: boolean; onOpen?: () => void; onStop: () => void;
}) {
  useTranslation(["briefings", "knowledge"]);
  return <DropdownMenu>
    <DropdownMenuTrigger asChild>
      <Button variant="ghost" size="icon-sm" className="following-row-menu" data-followed-menu disabled={disabled}
        aria-label={i18n.t("briefings:schedule.moreActions", { title: name })}><MoreHorizontalIcon /></Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end">
      {onOpen ? <DropdownMenuItem onSelect={onOpen}>{i18n.t("knowledge:map.openAssetPage")}</DropdownMenuItem> : null}
      <DropdownMenuItem data-followed-stop onSelect={onStop}>{i18n.t("knowledge:memoryPage.about.remove", { name })}</DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>;
}
