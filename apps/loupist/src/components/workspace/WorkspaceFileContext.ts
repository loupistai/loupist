import { createContext, useContext } from "react";
import type { LoupistBridge, WorkspaceFile } from "../../types";

/** File previews use the same scoped host reader as Files, never webview URLs. */
const WorkspaceFileContext = createContext<{ bridge: LoupistBridge; loupistId: string } | null>(null);
export const WorkspaceFileProvider = WorkspaceFileContext.Provider;
export const useWorkspaceFiles = () => useContext(WorkspaceFileContext);

export const workspaceRasterPreview = (file: WorkspaceFile): string | null =>
  !file.truncated && file.image_data_url && /^data:image\/(png|jpeg|gif|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(file.image_data_url)
    ? file.image_data_url : null;
