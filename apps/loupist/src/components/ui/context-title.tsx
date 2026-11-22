import type { ReactNode } from "react";
import "@/styles/context-title.css";

export const titlePlaceholder = "{{title}}";

/** Lets only the title shrink while the surrounding label keeps its full text. */
export function ContextTitle({ template, title, children }: { template: string; title: string; children?: ReactNode }) {
  const [before, after] = template.split(titlePlaceholder);
  return <span className="context-title" title={`${before}${title}${after ?? ""}`}>
    {before ? <span className="context-title-label">{before}</span> : null}
    <span className="context-title-name">{children ?? title}</span>
    {after ? <span className="context-title-label">{after}</span> : null}
  </span>;
}
