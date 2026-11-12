import { chatTitle } from "@/app/chatTitle";
import i18n, { Trans, useTranslation } from "@/i18n";
import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogBody,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  focusDialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { FolderOpenIcon, ShieldCheckIcon } from "lucide-react";
import { errorMessage } from "../lib/errors";
import type { ChatThread, LoupistCreateRequest, LoupistImportRequest } from "../types";
import type { WatchlistAddRequest } from "@/contracts/userContext";
import { LoupistTemplateFields, templateDraft, type LoupistDraft } from "@/components/onboarding/LoupistTemplateFields";
import "@/styles/sharing.css";

/** Draft state for the loupist/thread create, rename, and delete dialogs. */
export function useAppDialogs() {
  const [createLoupistOpen, setCreateLoupistOpen] = useState(false);
  const [importLoupistOpen, setImportLoupistOpen] = useState(false);
  const [reconnectLoupistOpen, setReconnectLoupistOpen] = useState(false);
  const [workspacePath, setWorkspacePath] = useState("");
  const [workspaceTrusted, setWorkspaceTrusted] = useState(false);
  const [renameThreadOpen, setRenameThreadOpen] = useState(false);
  const [loupistDraft, setLoupistDraft] = useState<LoupistDraft>({ template: "portfolio", avatar: "loupist-01", name: "", focus: "", tone: "balanced" });
  const [threadNameDraft, setThreadNameDraft] = useState("");
  const [threadToRename, setThreadToRename] = useState<ChatThread | null>(null);
  const [threadToDelete, setThreadToDelete] = useState<ChatThread | null>(null);
  const createLoupistGeneration = useRef(0);
  const createLoupistOpener = useRef<HTMLElement | null>(null);

  const openCreateLoupist = useCallback(() => {
    createLoupistOpener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    createLoupistGeneration.current += 1;
    setLoupistDraft(templateDraft("portfolio"));
    setWorkspacePath("");
    setWorkspaceTrusted(false);
    setCreateLoupistOpen(true);
  }, []);
  const openImportLoupist = useCallback(() => {
    createLoupistGeneration.current += 1;
    setWorkspacePath("");
    setWorkspaceTrusted(false);
    setImportLoupistOpen(true);
  }, []);
  const openReconnectLoupist = useCallback(() => {
    createLoupistGeneration.current += 1;
    setWorkspacePath("");
    setWorkspaceTrusted(false);
    setReconnectLoupistOpen(true);
  }, []);
  const openRenameThread = useCallback((thread: ChatThread) => {
    setThreadToRename(thread);
    setThreadNameDraft(chatTitle(thread));
    setRenameThreadOpen(true);
  }, []);
  const closeRenameThread = useCallback(() => {
    setRenameThreadOpen(false);
    setThreadToRename(null);
  }, []);
  const closeThreadDialogs = useCallback(() => {
    setRenameThreadOpen(false);
    setThreadToRename(null);
    setThreadToDelete(null);
  }, []);

  return {
    importLoupistOpen, setImportLoupistOpen, openImportLoupist,
    reconnectLoupistOpen, setReconnectLoupistOpen, openReconnectLoupist,
    workspacePath, setWorkspacePath, workspaceTrusted, setWorkspaceTrusted,
    createLoupistOpen,
    createLoupistGeneration,
    createLoupistOpener,
    setCreateLoupistOpen,
    renameThreadOpen,
    setRenameThreadOpen,
    loupistDraft,
    setLoupistDraft,
    threadNameDraft,
    setThreadNameDraft,
    threadToRename,
    threadToDelete,
    setThreadToDelete,
    openCreateLoupist,
    openRenameThread,
    closeRenameThread,
    closeThreadDialogs,
  };
}

export type AppDialogsState = ReturnType<typeof useAppDialogs>;

type AppDialogsProps = {
  dialogs: AppDialogsState;
  loupistMutationBusy: boolean;
  loupistMutationError: string | null;
  threadMutationBusy: boolean;
  threadMutationError: string | null;
  onChooseFolder: () => Promise<string | null>;
  onReconnectLoupist: (path: string) => Promise<boolean>;
  onImportLoupist: (request: LoupistImportRequest) => Promise<boolean>;
  onCreateLoupist: (request: LoupistCreateRequest) => Promise<string | null>;
  onRenameThread: (target: ChatThread, title: string) => Promise<boolean>;
  onDeleteThread: (thread: ChatThread) => Promise<boolean>;
  selectedLoupistName?: string;
  currentWorkspacePath?: string;
  /** "Use a link or file": opens the shared-Loupist link dialog (W19.4). */
  onUseLinkOrFile?: () => void;
  /** "Browse the Gallery" (W19.6); absent when no community index is configured. */
  onBrowseGallery?: () => void;
  /** A template's starter chip follows that asset right away. */
  onFollowStarter?: (entry: WatchlistAddRequest) => Promise<void>;
};

export function AppDialogs({
  dialogs,
  loupistMutationBusy,
  loupistMutationError,
  threadMutationBusy,
  threadMutationError,
  onCreateLoupist,
  onImportLoupist,
  onChooseFolder,
  onReconnectLoupist,
  onRenameThread,
  onDeleteThread,
  selectedLoupistName,
  currentWorkspacePath,
  onUseLinkOrFile,
  onBrowseGallery,
  onFollowStarter,
}: AppDialogsProps) {
  useTranslation();
  const {
    importLoupistOpen, setImportLoupistOpen,
    reconnectLoupistOpen, setReconnectLoupistOpen, workspacePath, setWorkspacePath, workspaceTrusted, setWorkspaceTrusted,
    createLoupistOpen,
    createLoupistGeneration,
    setCreateLoupistOpen,
    renameThreadOpen,
    setRenameThreadOpen,
    threadNameDraft,
    setThreadNameDraft,
    threadToRename,
    threadToDelete,
    setThreadToDelete,
    closeRenameThread,
  } = dialogs;

  const [choosingFolder, setChoosingFolder] = useState(false);
  const [folderError, setFolderError] = useState<string | null>(null);
  const folderRequest = useRef(0);
  useEffect(() => {
    folderRequest.current += 1;
    setChoosingFolder(false);
    setFolderError(null);
  }, [createLoupistOpen, reconnectLoupistOpen, importLoupistOpen]);
  const chooseFolder = async () => {
    const generation = createLoupistGeneration.current;
    const request = ++folderRequest.current;
    setChoosingFolder(true); setFolderError(null);
    try {
      const path = await onChooseFolder();
      if (request !== folderRequest.current || generation !== createLoupistGeneration.current || !path) return;
      setWorkspacePath(path); setWorkspaceTrusted(false);
    } catch (cause) { if (request === folderRequest.current && generation === createLoupistGeneration.current) setFolderError(errorMessage(cause, i18n.t("onboarding:appDialogs.couldNotChooseAFolder"))); }
    finally { if (request === folderRequest.current) setChoosingFolder(false); }
  };
  const folderControls = <section className="loupist-folder-section" aria-label={i18n.t("onboarding:appDialogs.workspaceSelection")}>
    <div className="loupist-folder-heading"><div><h3>{i18n.t("onboarding:appDialogs.localFolder")}</h3><p>{i18n.t(importLoupistOpen ? "onboarding:appDialogs.chooseTheFolderThatHoldsTheLoupist" : "onboarding:appDialogs.chooseTheOriginalFolderOrACopyOf")}</p></div><Button type="button" variant="outline" size="sm" disabled={loupistMutationBusy || choosingFolder} onClick={() => void chooseFolder()}><FolderOpenIcon />{choosingFolder ? i18n.t("onboarding:appDialogs.choosing") : workspacePath ? i18n.t("onboarding:appDialogs.changeFolder") : i18n.t("onboarding:appDialogs.chooseFolder")}</Button></div>
    {workspacePath ? <div className="loupist-folder-selection"><FolderOpenIcon aria-hidden="true" /><p className="workspace-path" aria-label={i18n.t("onboarding:appDialogs.selectedWorkspaceFolder")}>{workspacePath}</p></div> : null}
    {workspacePath ? <label className="workspace-trust loupist-folder-trust"><input type="checkbox" checked={workspaceTrusted} disabled={!workspacePath || loupistMutationBusy || choosingFolder} onChange={(event) => setWorkspaceTrusted(event.target.checked)} /><span><Trans ns="onboarding" i18nKey="appDialogs.workspaceTrust" components={[<strong />]} /></span></label> : null}
    {folderError ? <p className="inline-error" role="alert">{folderError}</p> : null}
  </section>;

  const preventComposingSubmit = (event: KeyboardEvent<HTMLFormElement>) => {
    if (event.key === "Enter" && (event.nativeEvent.isComposing || event.keyCode === 229)) event.preventDefault();
  };

  const submitRenameThread = async () => {
    if (!threadToRename || !threadNameDraft.trim() || threadMutationBusy) {
      return;
    }
    if (await onRenameThread(threadToRename, threadNameDraft)) {
      closeRenameThread();
    }
  };

  const submitDeleteThread = async () => {
    if (!threadToDelete || threadMutationBusy) return;
    if (await onDeleteThread(threadToDelete)) setThreadToDelete(null);
  };

  return (
    <>
      <Dialog open={createLoupistOpen} onOpenChange={open => { if (!loupistMutationBusy) setCreateLoupistOpen(open); }}>
        <DialogContent className="loupist-dialog loupist-workspace-dialog" showCloseButton={!loupistMutationBusy} onOpenAutoFocus={focusDialogTitle} onCloseAutoFocus={event => {
          const opener = dialogs.createLoupistOpener.current;
          const target = opener?.isConnected ? opener : document.querySelector<HTMLElement>(".loupist-header-new");
          if (target) { event.preventDefault(); target.focus({ preventScroll: true }); }
        }}>
          <form className="dialog-form" onKeyDown={preventComposingSubmit} onSubmit={event => {
            event.preventDefault();
            if (!dialogs.loupistDraft.name.trim() || loupistMutationBusy) return;
            const generation = createLoupistGeneration.current;
            void onCreateLoupist({ name: dialogs.loupistDraft.name.trim(), focus: dialogs.loupistDraft.focus.trim() || undefined, avatar: dialogs.loupistDraft.avatar, template: dialogs.loupistDraft.template, tone: dialogs.loupistDraft.tone }).then(created => {
              if (created && generation === createLoupistGeneration.current) setCreateLoupistOpen(false);
            });
          }}>
            <DialogHeader><DialogTitle>{i18n.t("onboarding:wizard.newLoupist")}</DialogTitle>
              <DialogDescription>{i18n.t("onboarding:wizard.newLoupistBody")}</DialogDescription></DialogHeader>
            <DialogBody><LoupistTemplateFields draft={dialogs.loupistDraft} onChange={dialogs.setLoupistDraft} busy={loupistMutationBusy} onFollow={onFollowStarter} />
              <p className="onboarding-background-note" data-background-reading>{i18n.t("onboarding:wizard.backgroundReading")}</p>
              {loupistMutationError && <div className="inline-error" role="alert">{loupistMutationError}</div>}</DialogBody>
            {onUseLinkOrFile ? <div className="loupist-shared-entry" data-loupist-shared-entry><span>{i18n.t("sharing:fromShared")}</span>
              {onBrowseGallery ? <Button type="button" variant="ghost" size="sm" disabled={loupistMutationBusy} data-browse-gallery onClick={onBrowseGallery}>{i18n.t("sharing:browseGallery")}</Button> : null}
              <Button type="button" variant="ghost" size="sm" disabled={loupistMutationBusy} data-use-link-or-file onClick={onUseLinkOrFile}>{i18n.t("sharing:useLinkOrFile")}</Button></div> : null}
            <DialogFooter><Button type="button" variant="outline" disabled={loupistMutationBusy} onClick={() => setCreateLoupistOpen(false)}>{i18n.t("onboarding:wizard.cancel")}</Button>
              <Button type="submit" disabled={loupistMutationBusy || !dialogs.loupistDraft.name.trim()}>{i18n.t(loupistMutationBusy ? "onboarding:wizard.creating" : "onboarding:wizard.create")}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <Dialog open={importLoupistOpen} onOpenChange={open => { if (!loupistMutationBusy) setImportLoupistOpen(open); }}>
        <DialogContent className="loupist-dialog loupist-workspace-dialog" showCloseButton={!loupistMutationBusy} onOpenAutoFocus={focusDialogTitle} data-loupist-import-dialog>
          <form className="dialog-form" onKeyDown={preventComposingSubmit} onSubmit={event => {
            event.preventDefault();
            if (!workspacePath || !workspaceTrusted || loupistMutationBusy || choosingFolder) return;
            const generation = createLoupistGeneration.current;
            void onImportLoupist({ workspace_path: workspacePath }).then(imported => {
              if (imported && generation === createLoupistGeneration.current) setImportLoupistOpen(false);
            });
          }}>
            <DialogHeader>
              <DialogTitle>{i18n.t("onboarding:appDialogs.importLoupistFolder")}</DialogTitle>
              <DialogDescription>{i18n.t("onboarding:appDialogs.importLoupistFolderDescription")}</DialogDescription>
            </DialogHeader>
            <DialogBody>
              {folderControls}
              {loupistMutationError ? <div role="alert" className="loupist-reconnect-error">
                <strong>{i18n.t("onboarding:appDialogs.couldNotImportThisFolder")}</strong>
                <details><summary>{i18n.t("onboarding:appDialogs.errorDetails")}</summary><p>{loupistMutationError}</p></details>
              </div> : null}
            </DialogBody>
            <DialogFooter>
              <Button type="button" variant="outline" disabled={loupistMutationBusy} onClick={() => setImportLoupistOpen(false)}>{i18n.t("onboarding:appDialogs.cancel")}</Button>
              <Button type="submit" disabled={!workspacePath || !workspaceTrusted || loupistMutationBusy || choosingFolder}>{i18n.t(loupistMutationBusy ? "onboarding:appDialogs.importing" : "onboarding:appDialogs.import")}</Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <Dialog open={reconnectLoupistOpen} onOpenChange={setReconnectLoupistOpen}>
        <DialogContent className="loupist-dialog loupist-workspace-dialog" onOpenAutoFocus={focusDialogTitle}>
          <DialogHeader><span className="loupist-dialog-eyebrow">{selectedLoupistName ?? i18n.t("onboarding:appDialogs.loupistWorkspace")}</span><DialogTitle>{i18n.t("onboarding:appDialogs.reconnectWorkspaceFolder")}</DialogTitle><DialogDescription>{i18n.t("onboarding:appDialogs.locateThisLoupistSOriginalOrCopiedFolder")}</DialogDescription></DialogHeader>
          <DialogBody>
            {currentWorkspacePath ? <div className="loupist-current-folder"><strong>{i18n.t("onboarding:appDialogs.currentLocation")}</strong><p className="workspace-path">{currentWorkspacePath}</p></div> : null}
            {folderControls}
            <div className="loupist-preserved-note"><ShieldCheckIcon aria-hidden="true" /><p>{i18n.t("onboarding:appDialogs.theFolderMustContainThisLoupistSSaved")}</p></div>
            {loupistMutationError ? <div role="alert" className="loupist-reconnect-error"><strong>{i18n.t("onboarding:appDialogs.couldNotReconnectThisFolder")}</strong><p>{i18n.t("onboarding:appDialogs.chooseThisLoupistSOriginalOrCopiedWorkspace")}</p><details><summary>{i18n.t("onboarding:appDialogs.errorDetails")}</summary><p>{loupistMutationError}</p></details></div> : null}
          </DialogBody>
          <DialogFooter><Button type="button" variant="outline" disabled={loupistMutationBusy} onClick={() => setReconnectLoupistOpen(false)}>{i18n.t("onboarding:appDialogs.cancel")}</Button><Button type="button" disabled={!workspacePath || !workspaceTrusted || loupistMutationBusy || choosingFolder} onClick={() => { const generation = createLoupistGeneration.current; void onReconnectLoupist(workspacePath).then((ok) => { if (ok && generation === createLoupistGeneration.current) setReconnectLoupistOpen(false); }); }}>{loupistMutationBusy ? i18n.t("onboarding:appDialogs.reconnecting") : i18n.t("onboarding:appDialogs.reconnectFolder")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog
        open={renameThreadOpen}
        onOpenChange={(open) => {
          if (threadMutationBusy) return;
          setRenameThreadOpen(open);
          if (!open) closeRenameThread();
        }}
      >
        <DialogContent size="sm" className="chat-dialog" showCloseButton={!threadMutationBusy}>
          <DialogHeader>
            <DialogTitle>{i18n.t("onboarding:appDialogs.renameChat")}</DialogTitle>
            <DialogDescription>
              {i18n.t("onboarding:appDialogs.updateTheChatNameShownUnderThisLoupist")}</DialogDescription>
          </DialogHeader>
          <div className="chat-dialog-body grid gap-2">
            <Label htmlFor="rename-thread-name">{i18n.t("onboarding:appDialogs.name")}</Label>
            <Input
              id="rename-thread-name"
              autoFocus
              disabled={threadMutationBusy}
              value={threadNameDraft}
              onChange={(event) => setThreadNameDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.nativeEvent.isComposing && event.keyCode !== 229) {
                  event.preventDefault();
                  void submitRenameThread();
                }
              }}
            />
          </div>
          {threadMutationError ? <div className="inline-error" role="alert">{threadMutationError}</div> : null}
          <DialogFooter>
            <Button
              variant="outline"
              disabled={threadMutationBusy}
              onClick={closeRenameThread}
            >
              {i18n.t("onboarding:appDialogs.cancel")}</Button>
            <Button
              disabled={!threadNameDraft.trim() || threadMutationBusy}
              onClick={() => void submitRenameThread()}
            >
              {threadMutationBusy ? i18n.t("onboarding:appDialogs.saving") : i18n.t("onboarding:appDialogs.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <Dialog
        open={threadToDelete !== null}
        onOpenChange={(open) => {
          if (!open && !threadMutationBusy) setThreadToDelete(null);
        }}
      >
        <DialogContent size="sm" className="chat-dialog" showCloseButton={!threadMutationBusy}>
          <DialogHeader>
            <DialogTitle>{i18n.t("onboarding:appDialogs.deleteChat")}</DialogTitle>
            <DialogDescription>
              {threadToDelete
                ? i18n.t("onboarding:appDialogs.andItsMessagesDocumentsAndChartsWill", { title: chatTitle(threadToDelete) })
                : i18n.t("onboarding:appDialogs.thisChatWillBePermanentlyDeleted")}
            </DialogDescription>
          </DialogHeader>
          {threadMutationError ? <div className="inline-error" role="alert">{threadMutationError}</div> : null}
          <DialogFooter>
            <Button
              variant="outline"
              disabled={threadMutationBusy}
              onClick={() => setThreadToDelete(null)}
            >
              {i18n.t("onboarding:appDialogs.cancel")}</Button>
            <Button
              variant="destructive"
              disabled={threadMutationBusy}
              onClick={() => void submitDeleteThread()}
            >
              {threadMutationBusy ? i18n.t("onboarding:appDialogs.deleting") : i18n.t("onboarding:appDialogs.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
