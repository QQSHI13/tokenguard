import { invoke } from "@tauri-apps/api/core";
import { sendNotification, isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";

const GITHUB_REPO = "QQSHI13/tokenguard";

type UpdateInfo = {
  version: string;
  asset_url: string;
  downloaded_path?: string;
};

async function ensureNotificationPermission() {
  const allowed = await isPermissionGranted();
  if (!allowed) {
    const permission = await requestPermission();
    return permission === "granted";
  }
  return true;
}

export async function checkForUpdate() {
  try {
    const info = await invoke<UpdateInfo | null>("check_for_update");
    if (!info) return;

    const path = await invoke<string>("download_update", { assetUrl: info.asset_url });

    await ensureNotificationPermission();
    sendNotification({
      title: "Token Guard — Update ready",
      body: `Version ${info.version} downloaded to ${path}`,
    });
  } catch (e) {
    console.error("Update check failed:", e);
  }
}
