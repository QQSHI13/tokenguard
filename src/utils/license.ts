const LICENSE_KEY = "tokenguard-license";
const DEVICE_ID_KEY = "tokenguard-device-id";

export const WORKER_URL = "https://tokenguard-license.qingquanshi65.workers.dev";

export function getLicenseKey(): string | null {
  try {
    return localStorage.getItem(LICENSE_KEY);
  } catch {
    return null;
  }
}

export function setLicenseKey(key: string) {
  try {
    localStorage.setItem(LICENSE_KEY, key);
  } catch {
    // ignore
  }
}

export function clearLicenseKey() {
  try {
    localStorage.removeItem(LICENSE_KEY);
  } catch {
    // ignore
  }
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

export function isLicensed(): boolean {
  return !!getLicenseKey();
}
