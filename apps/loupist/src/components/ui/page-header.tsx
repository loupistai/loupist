import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

type Props = {
  title: string;
  titleId?: string;
  description?: ReactNode;
  metadata?: ReactNode;
  actions?: ReactNode;
  className?: string;
};

export function PageHeader({ title, titleId, description, metadata, actions, className }: Props) {
  return (
    <header className={cn("page-header", className)}>
      <div className="page-header-copy">
        <h1 id={titleId}>{title}</h1>
        {description ? <p className="page-description">{description}</p> : null}
        {metadata ? <div className="page-metadata">{metadata}</div> : null}
      </div>
      {actions ? <div className="page-header-actions">{actions}</div> : null}
    </header>
  );
}
