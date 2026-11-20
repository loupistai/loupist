import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

type Props = {
  title: string;
  titleId?: string;
  /** The window's page header already shows the title: keep it for assistive technology only. */
  hideTitle?: boolean;
  description?: ReactNode;
  metadata?: ReactNode;
  actions?: ReactNode;
  className?: string;
};

export function PageHeader({ title, titleId, hideTitle = false, description, metadata, actions, className }: Props) {
  // The frame is the size container, so actions wrap under the title wherever the header is narrow (≤ 760 px), not only in a narrow window.
  return (
    <div className="page-header-frame"><header className={cn("page-header", className)} data-title-hidden={hideTitle || undefined}>
      <div className="page-header-copy">
        <h1 id={titleId} className={hideTitle ? "sr-only" : undefined}>{title}</h1>
        {description ? <p className="page-description">{description}</p> : null}
        {metadata ? <div className="page-metadata">{metadata}</div> : null}
      </div>
      {actions ? <div className="page-header-actions">{actions}</div> : null}
    </header></div>
  );
}
