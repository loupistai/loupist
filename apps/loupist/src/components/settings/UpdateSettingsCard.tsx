import { formatDate, formatPercent } from "../../i18n/format";
import i18n, { useTranslation, Trans } from "../../i18n";
import {
  CheckCircle2Icon,
  DownloadIcon,
  RefreshCwIcon,
  ShieldAlertIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import type { UpdatePhase, UpdateState } from "../../types";

type Props = {
  status: UpdateState | null;
  phase: UpdatePhase;
  error: string | null;
  onCheck: () => Promise<void>;
  onInstall: (offerId: string) => Promise<void>;
};

/**
 * The release time in milliseconds. The host sends RFC 3339; an older host sent
 * the updater's own form, `2026-11-14 8:50:41.0 +00:00:00`, which `Date.parse` rejects.
 */
const publishedAtMs = (value: string) => {
  const parsed = Date.parse(value);
  if (!Number.isNaN(parsed)) return parsed;
  const legacy = /^(\d{4})-(\d{2})-(\d{2}) (\d{1,2}):(\d{2}):(\d{2})(?:\.\d+)? ([+-])(\d{2}):(\d{2})(?::\d{2})?$/.exec(value.trim());
  if (!legacy) return null;
  const [, year, month, day, hour, minute, second, sign, offsetHours, offsetMinutes] = legacy;
  const offset = (Number(offsetHours) * 60 + Number(offsetMinutes)) * (sign === "-" ? -1 : 1);
  return Date.UTC(Number(year), Number(month) - 1, Number(day), Number(hour), Number(minute) - offset, Number(second));
};

/** A date only, in the interface language and this Mac's time zone. */
const displayPublishedAt = (value: string) => {
  const timestamp = publishedAtMs(value);
  if (timestamp === null) return null;
  // Korean reads the long form (2026년 11월 4일), not "2026. 11. 4.", as Schedule and sources do.
  return formatDate(timestamp, { dateStyle: i18n.language.toLowerCase().startsWith("ko") ? "long" : "medium" });
};

const blockerLabel = (kind: string) => {
  // The host's labels describe fixed activity kinds, not user-authored titles.
  switch (kind) {
    case "chat": return i18n.t("settings:updateSettingsCard.blockers.chat");
    case "chat_mutation": return i18n.t("settings:updateSettingsCard.blockers.chatChange");
    case "memory_write": return i18n.t("settings:updateSettingsCard.blockers.memoryWrite");
    case "memory_mutation": return i18n.t("settings:updateSettingsCard.blockers.memoryChange");
    case "loupist_mutation": return i18n.t("settings:updateSettingsCard.blockers.loupistChange");
    case "loupist_deletion": return i18n.t("settings:updateSettingsCard.blockers.loupistDeletion");
    case "marketplace_configuration": return i18n.t("settings:updateSettingsCard.blockers.marketplaceSettings");
    case "marketplace_credential": return i18n.t("settings:updateSettingsCard.blockers.marketplaceCredentials");
    case "provider_inference": return i18n.t("settings:updateSettingsCard.blockers.modelWork");
    case "provider_support": return i18n.t("settings:updateSettingsCard.blockers.modelConnection");
    case "graph": return i18n.t("settings:updateSettingsCard.blockers.map");
    case "update": return i18n.t("settings:updateSettingsCard.blockers.update");
    default: return i18n.t("settings:updateSettingsCard.blockers.other");
  }
};

export function UpdateSettingsCard({
  status: updateState,
  phase,
  error,
  onCheck,
  onInstall,
}: Props) {
  useTranslation("settings");
  const offer = updateState?.supported === true ? updateState.offer : null;
  const released = offer?.published_at ? displayPublishedAt(offer.published_at) : null;
  const upToDate =
    updateState?.supported === true &&
    !offer &&
    updateState?.last_checked_at_ms != null &&
    !error;
  const busy = phase !== "idle";
  const status =
    phase === "checking"
      ? i18n.t("settings:updateSettingsCard.checkingForUpdates")
      : phase === "confirming"
        ? i18n.t("settings:updateSettingsCard.waitingForYourApproval")
        : phase === "downloading"
          ? updateState?.total_bytes
            ? i18n.t("settings:updateSettingsCard.downloadProgress", { progress: formatPercent(Math.min(1, updateState.downloaded_bytes / updateState.total_bytes), { maximumFractionDigits: 0 }) })
            : i18n.t("settings:updateSettingsCard.downloadingAndVerifyingTheUpdate")
          : phase === "installing"
            ? i18n.t("settings:updateSettingsCard.installingTheVerifiedUpdate")
            : phase === "restarting"
              ? i18n.t("settings:updateSettingsCard.updateInstalledRestartingLoupist")
              : null;

  return (
    <Card className="update-card">
      <CardHeader>
        <CardTitle>{i18n.t("settings:updateSettingsCard.loupistTerminalUpdates")}</CardTitle>
        <CardDescription>
          {i18n.t("settings:updateSettingsCard.checkHereForNewVersionsNothingInstallsUntil")}</CardDescription>
      </CardHeader>
      <CardContent className="update-card-content">
        <dl className="update-version-grid">
          <div>
            <dt>{i18n.t("settings:updateSettingsCard.currentVersion")}</dt>
            <dd>{updateState?.current_version ?? i18n.t("settings:updateSettingsCard.loading")}</dd>
          </div>
          {offer?.version ? <div>
            <dt>{i18n.t("settings:updateSettingsCard.availableVersion")}</dt>
            <dd>{offer.version}</dd>
          </div> : null}
        </dl>

        {updateState?.supported === false ? (
          <div className="update-summary update-summary-warning">
            <ShieldAlertIcon aria-hidden="true" />
            <div>
              <strong>{i18n.t("settings:updateSettingsCard.automaticUpdatesAreUnavailable")}</strong>
              <p>{i18n.t("settings:updateSettingsCard.thisDevelopmentBuildDoesNotInstallUpdatesAutomatically")}</p>
            </div>
          </div>
        ) : upToDate ? (
          <div className="update-summary">
            <CheckCircle2Icon aria-hidden="true" />
            <div>
              <strong>{i18n.t("settings:updateSettingsCard.loupistTerminalIsUpToDate")}</strong>
              <p>{i18n.t("settings:updateSettingsCard.noNewerReleaseIsAvailable")}</p>
            </div>
          </div>
        ) : offer ? (
          <div className="update-release">
            <div className="update-summary">
              <DownloadIcon aria-hidden="true" />
              <div>
                <strong>{i18n.t("settings:updateSettingsCard.availableVersionNotice", { version: offer.version })}</strong>
                {released ? (
                  <p>
                    <Trans ns="settings" i18nKey="updateSettingsCard.released" values={{ date: released }} components={[<time dateTime={offer.published_at ?? undefined} />]} />
                  </p>
                ) : null}
              </div>
            </div>
            <section aria-labelledby="update-release-notes-title">
              <h3 id="update-release-notes-title">{i18n.t("settings:updateSettingsCard.releaseNotes")}</h3>
              <p>{offer.notes || i18n.t("settings:updateSettingsCard.noReleaseNotesWereProvided")}</p>
            </section>
          </div>
        ) : (
          <p className="update-placeholder">
            {i18n.t("settings:updateSettingsCard.checkForUpdatesToSeeTheInstalledAnd")}</p>
        )}

        <div className="update-actions">
          <Button
            type="button"
            variant="outline"
            disabled={busy}
            onClick={() => void onCheck()}
          >
            <RefreshCwIcon
              className={phase === "checking" ? "update-spin" : undefined}
              aria-hidden="true"
            />
            {phase === "checking" ? i18n.t("settings:updateSettingsCard.checking") : i18n.t("settings:updateSettingsCard.checkForUpdates")}
          </Button>
          {offer ? (
            <Button
              type="button"
              disabled={busy}
              onClick={() => void onInstall(offer.offer_id)}
            >
              <DownloadIcon aria-hidden="true" />
              {phase === "confirming"
                ? i18n.t("settings:updateSettingsCard.awaitingApproval")
                : phase === "downloading" || phase === "installing"
                  ? i18n.t("settings:updateSettingsCard.installing")
                : phase === "restarting"
                  ? i18n.t("settings:updateSettingsCard.restarting")
                  : i18n.t("settings:updateSettingsCard.installAndRestart")}
            </Button>
          ) : null}
        </div>

        {status ? (
          <p className="update-progress" role="status" aria-live="polite">
            {status}
          </p>
        ) : null}
        {updateState?.blockers.length ? (
          <div className="update-summary update-summary-warning" role="status">
            <ShieldAlertIcon aria-hidden="true" />
            <div>
              <strong>{i18n.t("settings:updateSettingsCard.finishActiveWorkBeforeUpdating")}</strong>
              <ul>
                {updateState.blockers.map((blocker) => (
                  <li key={`${blocker.kind}:${blocker.id}`}>{blockerLabel(blocker.kind)}</li>
                ))}
              </ul>
            </div>
          </div>
        ) : null}
        {error ? (
          <p className="inline-error" role="alert">
            {error}
          </p>
        ) : null}
      </CardContent>
    </Card>
  );
}
