import { useEffect, useState } from "react";
import { useTranslation } from "../../i18n";
import { useOpenProps } from "../navigation/Open";
import { useWorkspaceFiles, workspaceRasterPreview } from "./WorkspaceFileContext";

/** Only normalized workspace paths reach this component; Rust admits every read. */
export function WorkspaceImage({ path, alt }: { path: string; alt: string }) {
  const { t } = useTranslation("chat");
  const scope = useWorkspaceFiles();
  const open = useOpenProps({ kind: "file", path });
  const key = JSON.stringify([scope?.loupistId, path]);
  const [preview, setPreview] = useState<{ key: string; bridge: NonNullable<typeof scope>["bridge"]; src: string | null }>();
  const [failed, setFailed] = useState<string>();
  useEffect(() => {
    if (!scope) return;
    let active = true;
    setFailed(undefined);
    void scope.bridge.workspaceRead({ loupist_id: scope.loupistId, path })
      .then((file) => { if (active) setPreview({ key, bridge: scope.bridge, src: workspaceRasterPreview(file) }); })
      .catch(() => { if (active) setPreview({ key, bridge: scope.bridge, src: null }); });
    return () => { active = false; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the scope's fields; a new scope object for the same thread must not re-read.
  }, [scope?.bridge, scope?.loupistId, key, path]);
  const src = preview?.key === key && preview.bridge === scope?.bridge && failed !== key ? preview.src : null;
  const label = alt || path.split("/").at(-1) || path;
  const content = src ? <img src={src} alt={label} loading="lazy" onError={() => setFailed(key)} /> : <span>{label}</span>;
  return open.available ? <button type="button" className="workspace-inline-image" title={preview?.key === key && !src ? t("chatRail.previewUnavailable") : label} {...open.props}>{content}</button> : content;
}
