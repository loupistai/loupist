import * as React from "react";
import { cn } from "@/lib/utils";
import { fieldControlStyles } from "./field-styles";

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(fieldControlStyles,
        "h-[var(--gg-size-control-md)] px-2.5 py-1 file:inline-flex file:h-6 file:border-0 file:bg-transparent file:text-[length:var(--gg-text-control)] file:font-medium file:text-foreground disabled:bg-input/50",
        className)}
      {...props}
    />
  );
}

export { Input };
