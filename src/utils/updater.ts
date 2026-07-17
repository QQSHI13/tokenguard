import { invoke } from "@tauri-apps/api/core";
import {
  sendNotification,
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

export type UpdateInfo = {
  version: string;
  asset_url: string;
  /** Expected sha256 of the asset, if GitHub reported one. */
  digest?: string | null;
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

export async function checkForUpdate(): Promise<UpdateInfo | null> {
  return await invoke<UpdateInfo | null>("check_for_update");
}

export async function downloadUpdate(
  assetUrl: string,
  digest?: string | null,
): Promise<string> {
  return await invoke<string>("download_update", {
    assetUrl,
    digest: digest ?? null,
  });
}

export async function installUpdate(path: string): Promise<void> {
  await invoke("install_update", { path });
}

export async function runAutoUpdate() {
  try {
    const info = await checkForUpdate();
    if (!info) return;

    const path = await downloadUpdate(info.asset_url, info.digest);

    await ensureNotificationPermission();
    sendNotification({
      title: "Token Guard — Update ready",
      body: `Version ${info.version} downloaded to ${path}`,
    });
  } catch (e) {
    console.error("Auto update failed:", e);
  }
}
