import * as React from "react";
import { cn } from "@/lib/utils";
import { fieldControlStyles } from "./field-styles";

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(fieldControlStyles,
        "flex field-sizing-content min-h-16 resize-y px-2.5 py-1.5 leading-relaxed disabled:bg-input/50",
        className)}
      {...props}
    />
  );
}

export { Textarea };
