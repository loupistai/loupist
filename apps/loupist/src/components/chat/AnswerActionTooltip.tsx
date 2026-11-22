import type { ReactElement } from "react";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "../ui/tooltip";

export function AnswerActionTooltip({ label, children }: { label: string; children: ReactElement }) {
  return <TooltipProvider>
    <Tooltip disableHoverableContent>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent side="bottom" sideOffset={4} collisionPadding={4} hideWhenDetached updatePositionStrategy="always">
        {label}
      </TooltipContent>
    </Tooltip>
  </TooltipProvider>;
}
