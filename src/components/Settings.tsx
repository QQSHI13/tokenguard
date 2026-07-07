import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";
import License from "./License";
import { checkForUpdate, downloadUpdate, type UpdateInfo } from "../utils/updater";

type Settings = {
  port: number;
  budget: number;
  paused: boolean;
  proxy_url: string;
  provider_count: number;
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

  return (
    <div className="max-w-xl space-y-6 text-neutral-900 dark:text-neutral-100">
      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("proxyEndpoint")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          {t("proxyEndpointHelp")}
        </p>
        <div className="mt-3 flex items-center gap-2">
          <code className="flex-1 truncate rounded-md border border-neutral-300 bg-neutral-100 px-3 py-2 text-sm text-emerald-700 dark:border-neutral-700 dark:bg-neutral-950 dark:text-emerald-300">
            {settings?.proxy_url ?? "http://localhost:3742"}
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
      </section>

      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("limitsAndSubscriptions")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          {t("limitsAndSubscriptionsHelp")}
        </p>
      </section>

      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("language")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">{t("languageHelp") || "Choose the interface language."}</p>
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
        <h2 className="text-sm font-semibold">{t("port")}</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          {t("portHelp")}
        </p>
        <div className="mt-3 flex items-center gap-2">
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
          {t("updateCheckHelp") || "Check for a newer version from GitHub Releases."}
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
