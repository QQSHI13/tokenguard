import { useState, useEffect } from "react";
import {
  WORKER_URL,
  getLicenseKey,
  setLicenseKey,
  clearLicenseKey,
  getDeviceId,
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

  useEffect(() => {
    getLicenseKey().then(setStoredKey).catch(console.error);
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
      const res = await fetch(
        `${WORKER_URL}/api/license/validate?key=${encodeURIComponent(key.trim().toUpperCase())}&device=${encodeURIComponent(device)}`
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
      const res = await fetch(`${WORKER_URL}/api/license/register`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          key: key.trim().toUpperCase(),
          deviceFingerprint: device,
        }),
      });
      const data = await res.json();
      if (!res.ok) {
        showError(data.error || t("registrationFailed"));
        return;
      }
      await setLicenseKey(key.trim().toUpperCase());
      setStoredKey(key.trim().toUpperCase());
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
          key: current,
          deviceFingerprint: device,
        }),
      });
      await clearLicenseKey();
      setStoredKey(null);
      setKey("");
      onChange(false);
      showMessage(t("deviceUnregistered"));
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
              onChange={(e) => setKey(e.target.value)}
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
