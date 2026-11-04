import type { ComponentProps } from "react";
import { RadioGroup } from "radix-ui";
import { cn } from "@/lib/utils";

/** One choice or list filter, with arrow-key navigation and no fictitious tab panels. */
export function SegmentedControl({ className, ...props }: ComponentProps<typeof RadioGroup.Root>) {
  return <div data-slot="tabs" className={cn("flex flex-col gap-2", className)}>
    <RadioGroup.Root data-slot="tabs-list" orientation="horizontal"
      className="inline-flex min-h-9 w-fit flex-wrap items-center justify-center gap-1 rounded-lg p-1 text-muted-foreground" {...props} />
  </div>;
}

export function SegmentedControlItem({ className, ...props }: ComponentProps<typeof RadioGroup.Item>) {
  return <RadioGroup.Item data-slot="tabs-trigger"
    className={cn("inline-flex min-h-7 items-center justify-center rounded-md px-2.5 text-sm font-medium whitespace-nowrap transition-colors outline-none disabled:pointer-events-none disabled:opacity-50 data-[state=checked]:bg-[var(--gg-color-bg-selected)] data-[state=checked]:text-foreground", className)} {...props} />;
}
