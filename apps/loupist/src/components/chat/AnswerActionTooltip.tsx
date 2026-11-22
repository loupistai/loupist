import type { ReactElement } from "react";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "../ui/tooltip";

/**
 * The label of an action under an answer, drawn beside its button. It carries its own
 * provider, so an answer renders wherever it is mounted, with or without the shell's.
 */
export function AnswerActionTooltip({ label, children }: { label: string; children: ReactElement }) {
  return <TooltipProvider><Tooltip disableHoverableContent>
    <TooltipTrigger asChild>{children}</TooltipTrigger>
    <TooltipContent side="bottom" sideOffset={4} collisionPadding={4} hideWhenDetached updatePositionStrategy="always">
      {label}
    </TooltipContent>
  </Tooltip></TooltipProvider>;
}
