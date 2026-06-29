import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

type Settings = {
  port: number;
  budget: number;
  paused: boolean;
  proxy_url: string;
  provider_count: number;
} | null;

export default function SettingsTab({
  settings,
  onChanged,
}: {
  settings: Settings;
  onChanged: () => void;
}) {
  const { t, lang, setLang } = useI18n();
  const [port, setPort] = useState(String(settings?.port ?? 3742));
  const [copied, setCopied] = useState(false);

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
