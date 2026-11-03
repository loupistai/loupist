import type { ComponentProps } from "react";
import { ChevronDownIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { fieldControlStyles } from "./field-styles";

function NativeSelect({ className, ...props }: ComponentProps<"select">) {
  return (
    <span className="relative block min-w-0">
      <select
        data-slot="native-select"
        className={cn(fieldControlStyles, "h-[var(--gg-size-control-md)] appearance-none py-1 pr-8 pl-2.5", className)}
        {...props}
      />
      <ChevronDownIcon aria-hidden="true" className="pointer-events-none absolute top-1/2 right-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
    </span>
  );
}

export { NativeSelect };
