import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState, type ReactNode } from "react";
import type { DocumentRefusal, LoupistBridge, WorkspaceDocumentHandler, WorkspaceDocumentOpen } from "../../types";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";

const t = i18n.getFixedT(null, "chat");

function openRefusal(reason: DocumentRefusal) {
  const messages = {
    outside_workspace: t("workspaceFiles.documentOpen.outsideWorkspace"),
    unsupported_type: t("workspaceFiles.documentOpen.unsupportedType"),
    inaccessible_file: t("workspaceFiles.documentOpen.inaccessibleFile"),
    unsafe_document: t("workspaceFiles.documentOpen.unsafeDocument"),
  };
  return messages[reason];
}

export function WorkspaceFileActions({ bridge, loupistId, path, workbook, noPreview, loading, onAsk, onReveal, children }: {
  bridge: LoupistBridge; loupistId: string; path: string; workbook: boolean; noPreview: boolean;
  loading: boolean; onAsk: () => void; onReveal: () => void; children?: ReactNode;
}) {
  useTranslation("chat");
  const [handler, setHandler] = useState<WorkspaceDocumentHandler>();
  const [opening, setOpening] = useState(false);
  const [openResult, setOpenResult] = useState<WorkspaceDocumentOpen>();
  const [failure, setFailure] = useState<string>();
  const checkHandler = workbook || noPreview;
  useEffect(() => {
    if (!checkHandler) return;
    let active = true;
    void bridge.workspaceDocumentHandler({ loupist_id: loupistId, path }).then(result => {
      if (active) setHandler(result);
    }).catch(() => {
      if (active) setHandler(undefined);
    });
    return () => { active = false; };
  }, [bridge, loupistId, path, checkHandler]);

  const canOpen = workbook || (noPreview && handler?.status === "allowed");
  const launchFailure = openResult?.status === "handler_unavailable" ? openResult : undefined;
  const fallbackApp = handler?.status === "allowed" ? handler.application_name : null;
  const application = openResult?.status === "requested" ? openResult.application_name
    : launchFailure?.application_name ?? fallbackApp;
  const error = failure ?? (openResult?.status === "refused" ? openRefusal(openResult.reason)
    : launchFailure ? application
      ? t("workspaceFiles.documentOpen.failedIn", { application })
      : t("workspaceFiles.documentOpen.handlerUnavailable") : null);
  const open = async () => {
    setOpening(true);
    setFailure(undefined);
    setOpenResult(undefined);
    try {
      setOpenResult(await bridge.workspaceDocumentOpen({ loupist_id: loupistId, path }));
    } catch (cause) {
      setFailure(errorMessage(cause, t("workspaceFiles.documentOpen.couldNotOpen")));
    } finally {
      setOpening(false);
    }
  };

  return <>
    <div className="workspace-file-actions">
      {children}
      {canOpen ? <Button type="button" variant="outline" size="sm" disabled={opening} onClick={() => void open()}>
        {application ? t("workspaceFiles.documentOpen.openIn", { application }) : t("workspaceFiles.documentOpen.open")}
      </Button> : null}
      <Button type="button" variant="outline" size="sm" disabled={loading} onClick={onAsk}>{t("workspaceFiles.askAboutThis")}</Button>
    </div>
    {error ? <div className="workspace-file-state reader-error" role="alert">
      <p>{error}</p>
      {launchFailure?.reason ? <p>{launchFailure.reason}</p> : null}
      {launchFailure ? <Button type="button" variant="outline" size="sm" onClick={onReveal}>
        {t("workspaceFiles.showInFileManager")}
      </Button> : null}
    </div> : null}
  </>;
}
