import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

type Props = {
  title: string
  description?: ReactNode
  action?: ReactNode
  className?: string
}

function EmptyState({ title, description, action, className }: Props) {
  return (
    <div className={cn("empty-state", className)}>
      <h2>{title}</h2>
      {description ? <p>{description}</p> : null}
      {action ? <div className="empty-state-action">{action}</div> : null}
    </div>
  )
}

export { EmptyState }
