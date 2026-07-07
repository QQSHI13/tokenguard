import { useState, useEffect } from "react";
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
import { useI18n } from "../i18n";

export default function License({
  licensed,
  onChange,
}: {
  licensed: boolean;
  onChange: (licensed: boolean) => void;
}) {
  const { t } = useI18n();
  const [key, setKey] = useState("");
  const [storedKey, setStoredKey] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [devices, setDevices] = useState<RegisteredDevice[]>([]);
  const [maxDevices, setMaxDevices] = useState(2);

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

  const showError = (msg: string) => {
    setError(msg);
    setMessage(null);
  };

  const showMessage = (msg: string) => {
    setMessage(msg);
    setError(null);
  };

  const validate = async () => {
    if (!key.trim()) return;
    setLoading(true);
    setError(null);
    try {
      const device = await getDeviceId();
      const normalized = normalizeLicenseKey(key);
      const res = await fetch(
        `${WORKER_URL}/api/license/validate?key=${encodeURIComponent(normalized)}&device=${encodeURIComponent(device)}`
      );
      const data = await res.json();
      if (data.valid) {
        showMessage(
          t("keyIsValid", {
            devices: data.devices ?? 0,
            maxDevices: data.maxDevices ?? 2,
          }),
        );
      } else {
        showError(t("keyIsInvalid"));
      }
    } catch (e) {
      showError(t("couldNotReachLicenseServer"));
    } finally {
      setLoading(false);
    }
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
              onClick={validate}
              disabled={loading || !key.trim()}
              className="rounded-md bg-neutral-200 px-3 py-2 text-xs font-semibold text-neutral-800 hover:bg-neutral-300 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
            >
              {t("validate")}
            </button>
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
