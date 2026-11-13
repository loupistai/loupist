import i18n from "@/i18n";

/** Every file kind the native Save dialog (`export_save`) writes. */
export type ExportKind = "png" | "mp4" | "webm" | "md" | "csv";

/**
 * The native Save dialog's title in the interface language ("Save image" /
 * "이미지 저장"). The host draws the dialog but words nothing in it, as with
 * `pick_file`, so every export names its title here.
 */
export function exportTitle(kind: ExportKind): string {
  const key = kind === "mp4" || kind === "webm" ? "video" : kind;
  return i18n.t(`common:exportDialog.${key}`);
}

/** An export request with its dialog title, for `bridge.exportSave`. */
export const titledExport = <R extends { kind: ExportKind }>(request: R): R & { title: string } => ({ ...request, title: exportTitle(request.kind) });
