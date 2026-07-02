import { invoke } from "@tauri-apps/api/core";

const DEVICE_ID_KEY = "tokenguard-device-id";

export const WORKER_URL = "https://tokenguard-license.qingquanshi65.workers.dev";

export async function getLicenseKey(): Promise<string | null> {
  try {
    return await invoke<string | null>("get_license_key");
  } catch {
    return null;
  }
}

export async function setLicenseKey(key: string) {
  await invoke("set_license_key", { key });
}

export async function clearLicenseKey() {
  await invoke("delete_license_key");
}

export function getDeviceId(): string {
  try {
    let id = localStorage.getItem(DEVICE_ID_KEY);
    if (!id) {
      id = crypto.randomUUID();
      localStorage.setItem(DEVICE_ID_KEY, id);
    }
    return id;
  } catch {
    return "unknown";
  }
}

export async function isLicensed(): Promise<boolean> {
  return !!(await getLicenseKey());
}

export async function validateStoredKey(): Promise<boolean> {
  const key = await getLicenseKey();
  if (!key) return false;
  const device = getDeviceId();
  try {
    const res = await fetch(
      `${WORKER_URL}/api/license/validate?key=${encodeURIComponent(key)}&device=${encodeURIComponent(device)}`
    );
    const data = await res.json();
    if (!data.valid) {
      await clearLicenseKey();
      return false;
    }
    return true;
  } catch {
    // offline or worker unreachable: keep existing license state
    return true;
  }
}
