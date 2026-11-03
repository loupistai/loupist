import i18n from "@/i18n";
import { formatNumber } from "@/i18n/format";

const t = i18n.getFixedT(null, "chat");

/** One size label for every place a file appears: bytes, whole kilobytes rounded up, or megabytes to one decimal. */
export function fileSize(bytes: number) {
  if (bytes < 1024) return t("chatMessageCard.attachmentBytes", { bytes });
  if (bytes < 1024 * 1024) return t("chatMessageCard.attachmentKilobytes", { value1: Math.ceil(bytes / 1024) });
  return t("chatMessageCard.attachmentMegabytes", { value1: formatNumber(bytes / (1024 * 1024), { minimumFractionDigits: 1, maximumFractionDigits: 1 }) });
}
