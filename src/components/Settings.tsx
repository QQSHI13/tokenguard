import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";
import License from "./License";
import { checkForUpdate, downloadUpdate, installUpdate, type UpdateInfo } from "../utils/updater";

type Settings = {
  port: number;
  budget: number;
  paused: boolean;
  proxy_url: string;
  provider_count: number;
  log_bodies: boolean;
  auto_export_days: number;
  auto_export_folder: string | null;
  webhook_url: string | null;
  auto_start: boolean;
  key_rotation_days: number;
  log_retention_days: number;
  expose_to_lan: boolean;
  lan_ip: string | null;
} | null;

export default function SettingsTab({
  settings,
  licensed,
  onLicenseChange,
  onChanged,
}: {
  settings: Settings;
  licensed: boolean;
  onLicenseChange: (licensed: boolean) => void;
  onChanged: () => void;
}) {
  const { t, lang, setLang } = useI18n();
  const [port, setPort] = useState(String(settings?.port ?? 3742));
  const [copied, setCopied] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [updateStatus, setUpdateStatus] = useState<"idle" | "checking" | "downloading" | "done" | "error">("idle");
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [updatePath, setUpdatePath] = useState<string | null>(null);
  const [priceUrl, setPriceUrl] = useState("");
  const [priceStatus, setPriceStatus] = useState<"idle" | "refreshing" | "done" | "error">("idle");
  const [priceError, setPriceError] = useState<string | null>(null);
  const [priceCount, setPriceCount] = useState<number | null>(null);
  const [autoStart, setAutoStart] = useState(settings?.auto_start ?? false);
  const [autoStartStatus, setAutoStartStatus] = useState<string | null>(null);
  const [keyRotationDays, setKeyRotationDays] = useState(String(settings?.key_rotation_days ?? 90));
  const [keyRotationStatus, setKeyRotationStatus] = useState<string | null>(null);
  const [exposeToLan, setExposeToLan] = useState(settings?.expose_to_lan ?? false);
  const [lanIp, setLanIp] = useState(settings?.lan_ip ?? "");
  const [lanIpStatus, setLanIpStatus] = useState<string | null>(null);

  const savePort = async () => {
    await invoke("set_port", { port: Number(port) || 3742 });
    onChanged();
  };
  const runSelftest = async () => {
    const report = await invoke<string>("keyring_selftest");
    alert(report);
  };
  const copy = async () => {
    if (!settings) return;
    await navigator.clipboard.writeText(settings.proxy_url);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
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

  const handleRefreshPrices = async () => {
    if (!priceUrl.trim()) return;
    setPriceStatus("refreshing");
    setPriceError(null);
    setPriceCount(null);
    try {
      const count = await invoke<number>("refresh_model_prices_from_url", {
        url: priceUrl.trim(),
      });
      setPriceCount(count);
      setPriceStatus("done");
    } catch (e) {
      setPriceStatus("error");
      setPriceError(String(e));
    }
  };

  useEffect(() => {
    setAutoStart(settings?.auto_start ?? false);
  }, [settings?.auto_start]);

  useEffect(() => {
    setKeyRotationDays(String(settings?.key_rotation_days ?? 90));
  }, [settings?.key_rotation_days]);

  useEffect(() => {
    setExposeToLan(settings?.expose_to_lan ?? false);
  }, [settings?.expose_to_lan]);

  useEffect(() => {
    setLanIp(settings?.lan_ip ?? "");
  }, [settings?.lan_ip]);

  const handleSaveKeyRotation = async () => {
    setKeyRotationStatus(null);
    try {
      const days = Number(keyRotationDays) || 90;
      await invoke("set_key_rotation_days", { days });
      setKeyRotationStatus(t("keyRotationSaved"));
    } catch (e) {
      setKeyRotationStatus(String(e));
    }
  };

  const handleExposeToLanChange = async (enabled: boolean) => {
    try {
      await invoke("set_expose_to_lan", { enabled });
      setExposeToLan(enabled);
    } catch (e) {
      alert(String(e));
    }
  };

  const handleSaveLanIp = async () => {
    setLanIpStatus(null);
    try {
      const value = lanIp.trim() || null;
      await invoke("set_lan_ip", { ip: value });
      setLanIpStatus(t("lanIpSaved"));
      onChanged();
    } catch (e) {
      setLanIpStatus(String(e));
    }
  };

  const handleAutoStartChange = async (enabled: boolean) => {
    setAutoStartStatus(null);
    try {
      await invoke("set_auto_start", { enabled });
      setAutoStart(enabled);
      setAutoStartStatus(t("autoStartSaved"));
    } catch (e) {
      setAutoStartStatus(String(e));
    }
  };

  return (
    <div className="max-w-xl space-y-6 text-neutral-900 dark:text-neutral-100">
      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("proxyEndpoint")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          {t("proxyEndpointHelp")}
        </p>
        <div className="mt-3 flex items-center gap-2">
          <code className="flex-1 truncate rounded-md border border-neutral-300 bg-neutral-100 px-3 py-2 text-sm text-emerald-700 dark:border-neutral-700 dark:bg-neutral-950 dark:text-emerald-300">
            {settings?.proxy_url ?? "http://127.0.0.1:3742"}
          </code>
          <button
            onClick={copy}
            className="rounded-md bg-neutral-200 px-3 py-2 text-xs text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
          >
            {copied ? t("copied") : t("copy")}
          </button>
        </div>
        <p className="mt-2 text-[11px] text-neutral-600">
          {t("example")}:{" "}
          <code className="text-neutral-600 dark:text-neutral-400">
            OPENAI_BASE_URL={settings?.proxy_url}
          </code>
        </p>

        <label className="mt-4 block text-[11px] font-medium text-neutral-700 dark:text-neutral-300">
          {t("portLabel")}
        </label>
        <div className="mt-1 flex items-center gap-2">
          <input
            type="number"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            className="w-32 rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
          />
          <button
            onClick={savePort}
            className="rounded-md bg-neutral-200 px-3 py-1.5 text-xs text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
          >
            {t("save")}
          </button>
        </div>
        <p className="mt-1 text-[10px] text-neutral-500">
          {t("portHelp")}
        </p>

        <label className="mt-3 flex items-start gap-2 text-xs text-neutral-600 dark:text-neutral-400">
          <input
            type="checkbox"
            checked={exposeToLan}
            onChange={(e) => handleExposeToLanChange(e.target.checked)}
            className="mt-0.5"
          />
          <span>
            {t("exposeToLan")}
            <span className="block text-[10px] text-neutral-500">
              {t("exposeToLanHelp")}
            </span>
          </span>
        </label>

        {exposeToLan && (
          <div className="mt-3">
            <label className="block text-[11px] text-neutral-600 dark:text-neutral-400">
              {t("lanIpOverride")}
            </label>
            <div className="mt-1 flex items-center gap-2">
              <input
                type="text"
                value={lanIp}
                onChange={(e) => setLanIp(e.target.value)}
                placeholder={t("lanIpOverridePlaceholder")}
                className="flex-1 rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
              />
              <button
                onClick={handleSaveLanIp}
                className="rounded-md bg-neutral-200 px-3 py-1.5 text-xs text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
              >
                {t("save")}
              </button>
            </div>
            {lanIpStatus && (
              <p
                className={`mt-1 text-xs ${
                  lanIpStatus.includes(t("lanIpSaved")) ? "text-emerald-600" : "text-red-600"
                }`}
              >
                {lanIpStatus}
              </p>
            )}
          </div>
        )}
      </section>

      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("language")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">{t("languageHelp")}</p>
        <div className="mt-3">
          <select
            value={lang}
            onChange={(e) => setLang(e.target.value as "en" | "zh-CN")}
            className="w-40 rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
          >
            <option value="en">English</option>
            <option value="zh-CN">简体中文</option>
          </select>
        </div>
      </section>

      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("autoStart")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">{t("autoStartHelp")}</p>
        <label className="mt-3 flex items-center gap-2 text-xs text-neutral-400">
          <input
            type="checkbox"
            checked={autoStart}
            onChange={(e) => handleAutoStartChange(e.target.checked)}
          />
          {t("autoStartOnLogin")}
        </label>
        {autoStartStatus && (
          <p
            className={`mt-2 text-xs ${
              autoStartStatus.includes(t("autoStartSaved")) ? "text-emerald-600" : "text-red-600"
            }`}
          >
            {autoStartStatus}
          </p>
        )}
      </section>

      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("keyRotation")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">{t("keyRotationHelp")}</p>
        <div className="mt-3 flex items-center gap-2">
          <input
            type="number"
            min={1}
            value={keyRotationDays}
            onChange={(e) => setKeyRotationDays(e.target.value)}
            className="w-24 rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
          />
          <span className="text-xs text-neutral-500">{t("days")}</span>
          <button
            onClick={handleSaveKeyRotation}
            className="rounded-md bg-neutral-200 px-3 py-1.5 text-xs text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
          >
            {t("save")}
          </button>
        </div>
        {keyRotationStatus && (
          <p
            className={`mt-2 text-xs ${
              keyRotationStatus.includes(t("keyRotationSaved")) ? "text-emerald-600" : "text-red-600"
            }`}
          >
            {keyRotationStatus}
          </p>
        )}
      </section>

      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("keychain")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          {t("keychainHelp")}
        </p>
        <button
          onClick={runSelftest}
          className="mt-3 rounded-md bg-neutral-200 px-3 py-1.5 text-xs text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
        >
          {t("testKeychain")}
        </button>
      </section>

      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("updateCheck")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          {t("updateCheckHelp")}
        </p>
        <div className="mt-3 flex flex-wrap items-center gap-2">
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
      </section>

      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("priceDatabase")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          {t("priceDatabaseHelp")}
        </p>
        <div className="mt-3 flex items-center gap-2">
          <input
            type="text"
            value={priceUrl}
            onChange={(e) => setPriceUrl(e.target.value)}
            placeholder={t("priceDatabaseUrlPlaceholder")}
            className="flex-1 rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
          />
          <button
            onClick={handleRefreshPrices}
            disabled={priceStatus === "refreshing"}
            className="rounded-md bg-neutral-200 px-3 py-1.5 text-xs text-neutral-800 hover:bg-neutral-300 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
          >
            {priceStatus === "refreshing" ? t("working") : t("refreshPricesFromUrl")}
          </button>
        </div>
        {priceStatus === "done" && priceCount !== null && (
          <p className="mt-2 text-xs text-emerald-600">
            {t("pricesRefreshed", { count: priceCount })}
          </p>
        )}
        {priceStatus === "error" && priceError && (
          <p className="mt-2 text-xs text-red-600">
            {t("pricesRefreshFailed") + priceError}
          </p>
        )}
      </section>

      <License licensed={licensed} onChange={onLicenseChange} />

      <section className="rounded-lg border border-neutral-200 bg-white p-4 text-[11px] leading-relaxed text-neutral-500 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="mb-2 text-sm font-semibold text-neutral-700 dark:text-neutral-300">
          {t("privacySecurity")}
        </h2>
        <ul className="list-inside list-disc space-y-1">
          <li>{t("privacy1")}</li>
          <li>{t("privacy2")}</li>
          <li>{t("privacy3")}</li>
          <li>{t("privacy4")}</li>
        </ul>
      </section>
    </div>
  );
}
