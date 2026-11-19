import { openUrl } from "@tauri-apps/plugin-opener";

export function openExternal(url: string) {
  if ("__TAURI_INTERNALS__" in window) {
    void openUrl(url);
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
}
