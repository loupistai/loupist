import i18n, { useTranslation } from "@/i18n";
import type { FormEventHandler, RefObject } from "react";
import {
  ArrowLeftIcon,
  ArrowRightIcon,
  ExternalLinkIcon,
  RefreshCwIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import type { BrowserWorkspaceTab } from "../../chat/workspace";

const t = i18n.getFixedT(null, "chat");

type Props = {
  tab: BrowserWorkspaceTab;
  address: string;
  viewportRef: RefObject<HTMLDivElement | null>;
  onAddressChange: (address: string) => void;
  onNavigate: FormEventHandler<HTMLFormElement>;
  onAction: (action: "back" | "forward" | "reload") => void;
  onOpenExternal: (url: string) => void;
  onRetry: () => void;
  /** Present only when this failed page has a saved passage in the Evidence tab. */
  onViewSaved?: () => void;
};

export function BrowserWorkspaceView({
  tab,
  address,
  viewportRef,
  onAddressChange,
  onNavigate,
  onAction,
  onOpenExternal,
  onRetry,
  onViewSaved,
}: Props) {
  useTranslation("chat");
  const creating = tab.loading && !tab.native_id && !tab.error;
  return (
    <>
      <form className="browser-toolbar" onSubmit={onNavigate}>
        <div className="browser-history-actions">
          <Button
            type="button"
            size="icon"
            variant="ghost"
            aria-label={t("browserWorkspaceView.goBack")}
            disabled={!tab.native_id}
            onClick={() => onAction("back")}
          >
            <ArrowLeftIcon />
          </Button>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            aria-label={t("browserWorkspaceView.goForward")}
            disabled={!tab.native_id}
            onClick={() => onAction("forward")}
          >
            <ArrowRightIcon />
          </Button>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            aria-label={t("browserWorkspaceView.reloadPage")}
            disabled={!tab.native_id}
            onClick={() => onAction("reload")}
          >
            <RefreshCwIcon />
          </Button>
        </div>
        <input
          aria-label={t("browserWorkspaceView.webAddress")}
          value={address}
          placeholder="https://example.com"
          spellCheck={false}
          readOnly={creating}
          aria-busy={creating || undefined}
          onChange={(event) => onAddressChange(event.target.value)}
        />
        <Button type="submit" size="sm" disabled={creating} title={creating ? t("browserWorkspaceView.openingTheBrowserTab") : undefined}>
          {t("browserWorkspaceView.open")}</Button>
        <Button
          type="button"
          size="icon"
          variant="ghost"
          aria-label={t("browserWorkspaceView.openInSystemBrowser")}
          disabled={!tab.url}
          onClick={() => onOpenExternal(tab.url)}
        >
          <ExternalLinkIcon />
        </Button>
      </form>
      {tab.error && (
        <div className="browser-error" role="alert">
          <p>{tab.error}</p>
          <div className="browser-recovery-actions">
            <Button type="button" variant="outline" size="sm" disabled={!tab.url || tab.loading} onClick={onRetry}>{t("browserWorkspaceView.tryAgain")}</Button>
            <Button type="button" variant="outline" size="sm" disabled={!tab.url} onClick={() => onOpenExternal(tab.url)}>{t("browserWorkspaceView.openInBrowser")}</Button>
            {onViewSaved ? <Button type="button" variant="outline" size="sm" onClick={onViewSaved}>{t("browserWorkspaceView.viewSavedPassage")}</Button> : null}
          </div>
        </div>
      )}
      {onViewSaved && !tab.error ? (
        <div className="browser-excerpt-actions">
          <Button type="button" variant="outline" size="sm" onClick={onViewSaved}>{t("browserWorkspaceView.viewSavedPassage")}</Button>
        </div>
      ) : null}
      {!tab.url ? (
        <div className="browser-start">
          <h2>{t("browserWorkspaceView.browseInLoupist")}</h2>
          <p>
            {t("browserWorkspaceView.enterAnHTTPOrHTTPSAddressAboveSearchQueries")}</p>
        </div>
      ) : (
        <div className="browser-viewport" ref={viewportRef} role="group" aria-label={t("browserWorkspaceView.browserContent")}>
          {tab.loading && (
            <span className="browser-loading">
              <Spinner /> {t("browserWorkspaceView.loadingPage")}</span>
          )}
        </div>
      )}
    </>
  );
}
