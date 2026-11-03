import i18n, { useTranslation } from "@/i18n";
import { cn } from "@/lib/utils"
import { Loader2Icon } from "lucide-react"

function Spinner({ className, ...props }: React.ComponentProps<"svg">) {
  useTranslation();
  return (
    <Loader2Icon data-slot="spinner" role="status" aria-label={i18n.t("common:spinner.loading")} className={cn("size-4 animate-spin", className)} {...props} />
  )
}

export { Spinner }
