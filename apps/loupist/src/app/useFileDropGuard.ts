import { useEffect } from "react";

export function useFileDropGuard() {
  useEffect(() => {
    const preventFileNavigation = (event: DragEvent) => {
      const transfer = event.dataTransfer;
      if (transfer?.types?.includes("Files") || (transfer?.files?.length ?? 0) > 0) {
        event.preventDefault();
      }
    };
    // HTML file drops must not navigate the webview. Keep propagation so each
    // dropzone can still process files through its existing upload flow.
    window.addEventListener("dragover", preventFileNavigation, true);
    window.addEventListener("drop", preventFileNavigation, true);
    return () => {
      window.removeEventListener("dragover", preventFileNavigation, true);
      window.removeEventListener("drop", preventFileNavigation, true);
    };
  }, []);
}
