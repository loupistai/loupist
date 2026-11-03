import { ExternalLinkIcon } from "lucide-react";
import { useTranslation } from "@/i18n";
import type { ModelProviderOption } from "@/types";
import "@/styles/connect.css";

/** Which billing line a provider's credential needs, if any (providers spec §2.4). */
export function planLineKind(provider: ModelProviderOption | undefined): "chatgpt" | "chatgpt_limit" | "claude" | null {
  if (!provider || provider.status.credential_kind !== "plan") return null;
  if (provider.billing_note === "chatgpt_plan") return provider.status.notice?.kind === "plan_limit" ? "chatgpt_limit" : "chatgpt";
  if (provider.billing_note === "claude_extra_usage") return "claude";
  return null;
}

/**
 * "Using your ChatGPT plan · Manage usage" (or "Billed as Claude extra usage ·
 * Manage") next to a model control; while the plan is at its limit the line
 * says so in warning ink. Rust opens the fixed usage page.
 */
export function ProviderPlanLine({ provider, onOpenUsage, className }: {
  provider: ModelProviderOption | undefined;
  onOpenUsage: (provider: string) => void;
  className?: string;
}) {
  const { t } = useTranslation("settings");
  const kind = planLineKind(provider);
  if (!provider || !kind) return null;
  const text = kind === "claude" ? t("providers.claudeExtraShort") : kind === "chatgpt_limit" ? t("providers.planAtLimit") : t("providers.usingPlan");
  const link = kind === "claude" ? t("providers.manage") : t("providers.manageUsage");
  return <p className={["provider-plan-line", className].filter(Boolean).join(" ")} data-plan-line={kind}>
    <span data-tone={kind === "chatgpt_limit" ? "warning" : undefined}>{text}</span>
    <span aria-hidden="true">·</span>
    <button type="button" className="connect-link" onClick={() => onOpenUsage(provider.id)}>{link}<ExternalLinkIcon aria-hidden="true" /></button>
  </p>;
}
