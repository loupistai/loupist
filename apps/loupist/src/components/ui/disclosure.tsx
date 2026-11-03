import { useState, type ComponentProps, type ReactNode } from "react";
import { ChevronRightIcon } from "lucide-react";
import { cn } from "@/lib/utils";

type Props = Omit<ComponentProps<"details">, "title" | "onToggle" | "open"> & {
  title: ReactNode;
  description?: ReactNode;
  defaultOpen?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
};

/** Secondary options stay mounted so collapsing a section preserves its draft. */
export function Disclosure({ title, description, defaultOpen = false, open, onOpenChange, className, children, ...props }: Props) {
  const [expanded, setExpanded] = useState(defaultOpen);
  return <details className={cn("ui-disclosure", className)} open={open ?? expanded}
    onToggle={(event) => {
      const next = event.currentTarget.open;
      setExpanded(next);
      if (next !== (open ?? expanded)) onOpenChange?.(next);
    }} {...props}>
    <summary><ChevronRightIcon aria-hidden="true" /><span>{title}{description ? <small>{description}</small> : null}</span></summary>
    <div className="ui-disclosure-body">{children}</div>
  </details>;
}
