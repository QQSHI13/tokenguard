import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  WORKER_URL,
  getLicenseKey,
  setLicenseKey,
  clearLicenseKey,
  getDeviceId,
  normalizeLicenseKey,
  formatLicenseKey,
  type RegisteredDevice,
} from "../utils/license";
import {
  checkForUpdate,
  downloadUpdate,
  installUpdate,
  type UpdateInfo,
} from "../utils/updater";
import { useI18n } from "../i18n";

type Settings = {
  auto_update_interval_minutes: number;
} | null;

export default function License({
  licensed,
  onChange,
  settings,
  onSettingsChanged,
}: {
  licensed: boolean;
  onChange: (licensed: boolean) => void;
  settings?: Settings;
  onSettingsChanged?: () => void;
}) {
  const { t } = useI18n();
  const [key, setKey] = useState("");
  const [storedKey, setStoredKey] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [devices, setDevices] = useState<RegisteredDevice[]>([]);
  const [maxDevices, setMaxDevices] = useState(2);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [updateStatus, setUpdateStatus] = useState<"idle" | "checking" | "downloading" | "done" | "error">("idle");
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [updatePath, setUpdatePath] = useState<string | null>(null);
  const [intervalMinutes, setIntervalMinutes] = useState(String(settings?.auto_update_interval_minutes ?? 240));

  const loadStoredKey = async () => {
    const k = await getLicenseKey();
    setStoredKey(k ? formatLicenseKey(k) : null);
  };

  const loadDevices = async () => {
    const raw = await getLicenseKey();
    if (!raw) {
      setDevices([]);
      return;
    }
    try {
      const device = await getDeviceId();
      const res = await fetch(
        `${WORKER_URL}/api/license/devices?key=${encodeURIComponent(normalizeLicenseKey(raw))}&device=${encodeURIComponent(device)}`
      );
      const data = await res.json();
      if (res.ok) {
        setDevices(data.devices ?? []);
        setMaxDevices(data.maxDevices ?? 2);
      }
    } catch (e) {
      console.error("Failed to load devices", e);
    }
  };

  useEffect(() => {
    loadStoredKey().catch(console.error);
    loadDevices().catch(console.error);
  }, [licensed]);

  useEffect(() => {
    setIntervalMinutes(String(settings?.auto_update_interval_minutes ?? 240));
  }, [settings?.auto_update_interval_minutes]);

  const showError = (msg: string) => {
    setError(msg);
    setMessage(null);
  };

  const showMessage = (msg: string) => {
    setMessage(msg);
    setError(null);
  };

  const register = async () => {
    if (!key.trim()) return;
    setLoading(true);
    setError(null);
    try {
      const device = await getDeviceId();
      const normalized = normalizeLicenseKey(key);
      const res = await fetch(`${WORKER_URL}/api/license/register`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          key: normalized,
          deviceFingerprint: device,
        }),
      });
      const data = await res.json();
      if (!res.ok) {
        showError(data.error || t("registrationFailed"));
        return;
      }
      await setLicenseKey(normalized);
      setStoredKey(formatLicenseKey(normalized));
      onChange(true);
      showMessage(
        t("registeredThisDevice", {
          devices: data.devices ?? 1,
          maxDevices: data.maxDevices ?? 2,
        }),
      );
    } catch (e) {
      showError(t("couldNotReachLicenseServer"));
    } finally {
      setLoading(false);
    }
  };

  const unregister = async () => {
    const current = storedKey;
    if (!current) return;
    setLoading(true);
    setError(null);
    try {
      const device = await getDeviceId();
      await fetch(`${WORKER_URL}/api/license/unregister`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          key: normalizeLicenseKey(current),
          deviceFingerprint: device,
        }),
      });
      await clearLicenseKey();
      setStoredKey(null);
      setKey("");
      setDevices([]);
      onChange(false);
      showMessage(t("deviceUnregistered"));
    } catch (e) {
      showError(t("couldNotReachLicenseServer"));
    } finally {
      setLoading(false);
    }
  };

  const handleCheckUpdate = async () => {
    setUpdateStatus("checking");
    setUpdateError(null);
    setUpdateInfo(null);
    setUpdatePath(null);
    try {
      const info = await checkForUpdate();
      if (info) {
        setUpdateInfo(info);
        setUpdateStatus("idle");
      } else {
        setUpdateStatus("done");
      }
    } catch (e) {
      setUpdateStatus("error");
      setUpdateError(String(e));
    }
  };

  const handleDownloadUpdate = async () => {
    if (!updateInfo) return;
    setUpdateStatus("downloading");
    try {
      const path = await downloadUpdate(updateInfo.asset_url);
      setUpdatePath(path);
      setUpdateStatus("done");
    } catch (e) {
      setUpdateStatus("error");
      setUpdateError(String(e));
    }
  };

  const handleInstallUpdate = async () => {
    if (!updatePath) return;
    try {
      await installUpdate(updatePath);
    } catch (e) {
      setUpdateStatus("error");
      setUpdateError(String(e));
    }
  };

  const handleSaveInterval = async () => {
    const minutes = Number(intervalMinutes);
    if (!Number.isFinite(minutes) || minutes < 0) {
      showError(t("invalidInterval"));
      return;
    }
    try {
      await invoke("set_auto_update_interval_minutes", { minutes: Math.round(minutes) });
      onSettingsChanged?.();
      showMessage(t("intervalSaved"));
    } catch (e) {
      showError(String(e));
    }
  };

  const unregisterOther = async (fingerprint: string) => {
    const current = storedKey;
    if (!current) return;
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(`${WORKER_URL}/api/license/unregister`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          key: normalizeLicenseKey(current),
          deviceFingerprint: fingerprint,
        }),
      });
      if (!res.ok) {
        const data = await res.json();
        showError(data.error || t("unregisterFailed"));
        return;
      }
      showMessage(t("deviceUnregistered"));
      await loadDevices();
    } catch (e) {
      showError(t("couldNotReachLicenseServer"));
    } finally {
      setLoading(false);
    }
  };

  return (
    <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
      <h2 className="text-sm font-semibold">{t("licenseKey")}</h2>
      <p className="mt-1 text-[11px] text-neutral-500">
        {t("licenseKeyHelp")}
      </p>

      {licensed && storedKey ? (
        <div className="mt-4 space-y-3">
          <p className="text-sm text-emerald-600">{t("licensedOnThisDevice")}</p>
          <code className="block truncate rounded-md border border-neutral-300 bg-neutral-100 px-3 py-2 text-sm text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100">
            {storedKey}
          </code>
          <p className="text-xs text-neutral-500">
            {t("registeredDevices", { count: devices.length, max: maxDevices })}
          </p>
          {devices.length > 0 && (
            <ul className="space-y-2">
              {devices.map((d, i) => (
                <li
                  key={d.fingerprint}
                  className="flex items-center justify-between rounded-md border border-neutral-200 bg-neutral-50 px-3 py-2 text-xs dark:border-neutral-800 dark:bg-neutral-950"
                >
                  <span className="text-neutral-700 dark:text-neutral-300">
                    {d.current ? t("thisDevice") : t("deviceN", { n: i + 1 })}
                    {d.registeredAt && (
                      <span className="ml-2 text-neutral-400">
                        {new Date(d.registeredAt).toLocaleDateString()}
                      </span>
                    )}
                  </span>
                  {!d.current && (
                    <button
                      onClick={() => unregisterOther(d.fingerprint)}
                      disabled={loading}
                      className="rounded-md bg-red-600 px-2 py-1 text-xs font-semibold text-white hover:bg-red-700 disabled:opacity-50"
                    >
                      {t("unregister")}
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
          <button
            onClick={unregister}
            disabled={loading}
            className="rounded-md bg-red-600 px-3 py-2 text-xs font-semibold text-white hover:bg-red-700 disabled:opacity-50"
          >
            {loading ? t("working") : t("unregisterThisDevice")}
          </button>

          <div className="rounded-md border border-neutral-200 bg-white p-3 dark:border-neutral-800 dark:bg-neutral-950">
            <h3 className="text-xs font-semibold text-neutral-800 dark:text-neutral-200">
              {t("updateCheck")}
            </h3>
            <p className="mt-1 text-[10px] text-neutral-500 dark:text-neutral-400">
              {t("updateCheckLicensedHelp")}
            </p>
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <button
                onClick={handleCheckUpdate}
                disabled={updateStatus === "checking" || updateStatus === "downloading"}
                className="rounded-md bg-neutral-200 px-3 py-1.5 text-xs text-neutral-800 hover:bg-neutral-300 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
              >
                {updateStatus === "checking" ? t("updateChecking") : t("checkForUpdates")}
              </button>
              {updateInfo && (
                <button
                  onClick={handleDownloadUpdate}
                  disabled={updateStatus === "downloading"}
                  className="rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-emerald-700 disabled:opacity-50"
                >
                  {updateStatus === "downloading" ? t("working") : t("downloadUpdate")}
                </button>
              )}
              {updatePath && (
                <button
                  onClick={handleInstallUpdate}
                  className="rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-emerald-700"
                >
                  {t("installUpdate")}
                </button>
              )}
            </div>
            {updateInfo && updateStatus !== "downloading" && updateStatus !== "done" && (
              <p className="mt-2 text-xs text-emerald-600">
                {t("updateAvailable", { version: updateInfo.version })}
              </p>
            )}
            {updateStatus === "done" && !updatePath && (
              <p className="mt-2 text-xs text-emerald-600">{t("updateUpToDate")}</p>
            )}
            {updatePath && (
              <p className="mt-2 text-xs text-emerald-600">
                {t("updateDownloaded", { path: updatePath })}
              </p>
            )}
            {updateStatus === "error" && updateError && (
              <p className="mt-2 text-xs text-red-600">
                {t("updateCheckFailed", { error: updateError })}
              </p>
            )}

            <div className="mt-3 flex items-center gap-2">
              <label className="text-[10px] text-neutral-500 dark:text-neutral-400">
                {t("autoCheckInterval")}
              </label>
              <input
                type="number"
                min={0}
                step={15}
                value={intervalMinutes}
                onChange={(e) => setIntervalMinutes(e.target.value)}
                className="w-20 rounded-md border border-neutral-300 bg-white px-2 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
              />
              <span className="text-[10px] text-neutral-500 dark:text-neutral-400">{t("minutes")}</span>
              <button
                onClick={handleSaveInterval}
                className="rounded-md bg-neutral-200 px-2 py-1 text-[10px] font-semibold text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
              >
                {t("save")}
              </button>
            </div>
            <p className="text-[10px] text-neutral-500 dark:text-neutral-400">
              {t("autoCheckIntervalHelp")}
            </p>
          </div>
        </div>
      ) : (
        <div className="mt-4 space-y-3">
          <div className="flex items-center gap-2">
            <input
              type="text"
              value={key}
              onChange={(e) => {
                const raw = e.target.value.replace(/[^0-9a-zA-Z]/g, '').toUpperCase().slice(0, 16);
                const parts = [
                  raw.slice(0, 4),
                  raw.slice(4, 8),
                  raw.slice(8, 12),
                  raw.slice(12, 16),
                ].filter(Boolean);
                setKey(parts.join('-'));
              }}
              placeholder={t("licenseKeyPlaceholder")}
              className="flex-1 rounded-md border border-neutral-300 bg-white px-3 py-2 text-sm text-neutral-900 placeholder-neutral-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100"
            />
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              onClick={register}
              disabled={loading || !key.trim()}
              className="rounded-md bg-emerald-600 px-3 py-2 text-xs font-semibold text-white hover:bg-emerald-700 disabled:opacity-50"
            >
              {loading ? t("working") : t("activateThisDevice")}
            </button>
          </div>
        </div>
      )}

      {error && <p className="mt-3 text-xs text-red-600">{error}</p>}
      {message && <p className="mt-3 text-xs text-emerald-600">{message}</p>}
    </section>
  );
}
