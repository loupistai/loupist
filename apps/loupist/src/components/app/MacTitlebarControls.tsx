import i18n, { useTranslation } from "@/i18n";
import { SidebarTrigger } from "@/components/ui/sidebar";

export function MacTitlebarControls() {
  useTranslation();
  return (
    <div className="macos-titlebar-controls">
      <SidebarTrigger
        className="macos-titlebar-sidebar-trigger"
        aria-label={i18n.t("app:macTitlebarControls.showOrHideSidebar")}
        title={i18n.t("app:macTitlebarControls.sidebarShortcut", { shortcut: "⌘B" })}
      />
    </div>
  );
}
