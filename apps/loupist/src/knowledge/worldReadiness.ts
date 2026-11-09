import i18n from "../i18n";
import type { GraphProcessingView } from "../types";

export type WorldReadinessStatus = GraphProcessingView["readiness"]["status"];
export const worldReadinessLabels = (): Record<GraphProcessingView["readiness"]["status"], string> => ({
  ready: i18n.t("knowledge:worldProcessingSettings.readyToProcess"),
  disabled: i18n.t("knowledge:worldProcessingSettings.automaticProcessingIsOff"),
  paused: i18n.t("knowledge:worldProcessingSettings.processingIsPaused"),
  model_required: i18n.t("knowledge:worldProcessingSettings.chooseAModel"),
  provider_disconnected: i18n.t("knowledge:worldProcessingSettings.reconnectProvider"),
  runtime_unavailable: i18n.t("knowledge:worldProcessingSettings.processingIsUnavailable"),
  model_unavailable: i18n.t("knowledge:worldProcessingSettings.modelIsUnavailable"),
  reconciliation_required: i18n.t("knowledge:worldProcessingSettings.recoveryRequired"),
  recovering: i18n.t("knowledge:worldProcessingSettings.recoveringWorldProcessing"),
});
