import { formatNumber } from "@/i18n/format";
import i18n from "@/i18n";

const t = i18n.getFixedT(null, "chat");
export const readerValue = (value: unknown): string => {
  if (value === null || value === undefined) return t("readerFormatting.notSpecified");
  if (typeof value === "number") return formatNumber(value);
  if (typeof value === "boolean") return value ? t("readerFormatting.yes") : t("readerFormatting.no");
  if (Array.isArray(value) && value.every((entry) => entry === null || typeof entry !== "object")) return value.map(readerValue).join(", ") || t("readerFormatting.none");
  return typeof value === "object" ? JSON.stringify(value) : String(value);
};
