import i18n, { useTranslation } from "@/i18n";
import { RefreshCwIcon, Trash2Icon } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { LoupistAvailability } from "../../types";

type Props = {
  availability: LoupistAvailability;
  busy: boolean;
  error: string | null;
  onRecover: () => void;
  onReconnect?: () => void;
  /** Removes the Loupist from the app (its folder stays); the only way past an earlier- or newer-version folder. */
  onRemove?: () => void;
  className?: string;
  compact?: boolean;
};

export function LoupistAvailabilityBoundary({
  availability,
  busy,
  error,
  onRecover,
  onReconnect,
  onRemove,
  className = "loupist-onboarding",
  compact = false,
}: Props) {
  useTranslation();
  const headingId = useId();
  if (availability.status === "available") return null;

  const Heading = compact ? "h3" : "h1";
  if (availability.status === "earlier_version" || availability.status === "newer_version") {
    // Rust refuses a pre-upgrade development format and leaves a folder a newer
    // Loupist saved untouched; neither is a moved folder, so there is no reconnect.
    const newer = availability.status === "newer_version";
    return (
      <section className={className} role="alert" aria-labelledby={headingId}>
        <Heading id={headingId}>{newer ? i18n.t("app:loupistAvailabilityBoundary.newerVersion") : i18n.t("app:loupistAvailabilityBoundary.earlierVersion")}</Heading>
        <p>{newer ? i18n.t("app:loupistAvailabilityBoundary.newerVersionBody") : i18n.t("app:loupistAvailabilityBoundary.earlierVersionBody")}</p>
        {error ? <div className="inline-error">{error}</div> : null}
        {onRemove ? <div className="onboarding-actions">
          <Button type="button" variant="outline" disabled={busy} onClick={onRemove}>
            <Trash2Icon aria-hidden="true" />
            {i18n.t("app:loupistAvailabilityBoundary.removeLoupist")}
          </Button>
        </div> : null}
      </section>
    );
  }
  const unavailable = availability.status === "workspace_unavailable";
  return (
    <section
      className={className}
      role="alert"
      aria-labelledby={headingId}
    >
      <Heading id={headingId}>{unavailable ? i18n.t("app:loupistAvailabilityBoundary.workspaceFolderUnavailable") : i18n.t("app:loupistAvailabilityBoundary.memoryNeedsRecovery")}</Heading>
      <p>
        {unavailable ? i18n.t("app:loupistAvailabilityBoundary.thisLoupistSFolderWasMovedOrIs") : i18n.t("app:loupistAvailabilityBoundary.anInterruptedUpdateWasSavedRecoverMemoryTo")}
      </p>
      {error ? <div className="inline-error">{error}</div> : null}
      <div className="onboarding-actions">
        <Button type="button" disabled={busy} onClick={unavailable ? onReconnect : onRecover}>
          <RefreshCwIcon aria-hidden="true" />
          {unavailable ? i18n.t("app:loupistAvailabilityBoundary.reconnectFolder") : busy ? i18n.t("app:loupistAvailabilityBoundary.recovering") : i18n.t("app:loupistAvailabilityBoundary.recoverMemory")}
        </Button>
      </div>
    </section>
  );
}
import { useId } from "react";
