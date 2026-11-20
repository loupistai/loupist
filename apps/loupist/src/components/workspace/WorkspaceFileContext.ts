import { createContext, useContext } from "react";
import type { LoupistBridge, WorkspaceFile } from "../../types";

/** File previews use the same scoped host reader as Files, never webview URLs. */
const WorkspaceFileContext = createContext<{ bridge: LoupistBridge; loupistId: string } | null>(null);
export const WorkspaceFileProvider = WorkspaceFileContext.Provider;
export const useWorkspaceFiles = () => useContext(WorkspaceFileContext);

export const workspaceRasterPreview = (file: WorkspaceFile): string | null =>
  !file.truncated && file.image_data_url && /^data:image\/(png|jpeg|gif|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(file.image_data_url)
    ? file.image_data_url : null;

/**
 * A complete SVG read as an image resource: a data URL of the file's text for an `<img>`,
 * where its scripts do not run and its external references do not load. Its markup is
 * never put in the DOM, an iframe or an object. A truncated read is never drawn.
 */
export const workspaceSvgPreview = (file: WorkspaceFile): string | null =>
  !file.truncated && !file.binary && typeof file.content === "string" && /\.svg$/i.test(file.path)
    ? `data:image/svg+xml;charset=utf-8,${encodeURIComponent(file.content)}` : null;
