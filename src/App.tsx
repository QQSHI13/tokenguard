import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import Dashboard from "./components/Dashboard";
import SettingsTab from "./components/Settings";
import Config from "./components/Config";
import Docs from "./components/Docs";
import ThemeToggle, { initTheme } from "./components/ThemeToggle";
import Banner from "./components/Banner";
import { useI18n } from "./i18n";
import {
  isLicensed,
} from "./utils/license";
import { runAutoUpdate } from "./utils/updater";

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
  auto_update_interval_minutes: number;
};
type Spend = { today: number; budget: number };

type Tab = "dashboard" | "config" | "settings" | "docs";

export default function App() {
  const { t } = useI18n();
  const [tab, setTab] = useState<Tab>("dashboard");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [spend, setSpend] = useState<Spend>({ today: 0, budget: 0 });
  const [tick, setTick] = useState(0);
  const [licensed, setLicensed] = useState(false);

  const refresh = useCallback(() => {
    invoke<Settings>("get_settings").then(setSettings).catch(console.error);
    invoke<Spend>("get_today_spend").then(setSpend).catch(console.error);
  }, []);

  useEffect(() => {
    isLicensed().then(setLicensed).catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
    const i = setInterval(refresh, 4000);
    return () => clearInterval(i);
  }, [refresh, tick]);

  useEffect(() => {
    return initTheme();
  }, []);

  useEffect(() => {
    const minutes = settings?.auto_update_interval_minutes ?? 240;
    if (!licensed || minutes === 0) return;
    runAutoUpdate();
    const i = setInterval(runAutoUpdate, minutes * 60 * 1000);
    return () => clearInterval(i);
  }, [licensed, settings?.auto_update_interval_minutes]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    listen<string>("set_tab", (event) => {
      const t = event.payload as Tab;
      if (
        ["dashboard", "config", "settings", "docs"].includes(t)
      ) {
        setTab(t);
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const togglePause = async () => {
    const paused = await invoke<boolean>("pause_resume");
    setSettings((s) => (s ? { ...s, paused } : s));
  };

  const tabs: { id: Tab; label: string }[] = [
    { id: "dashboard", label: t("dashboard") },
    { id: "config", label: t("config") },
    { id: "settings", label: t("settings") },
    { id: "docs", label: t("docs") },
  ];

  return (
    <div className="flex h-full bg-white text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
      <aside className="flex w-48 shrink-0 flex-col border-r border-neutral-200 dark:border-neutral-800">
        <div className="flex items-center gap-2.5 border-b border-neutral-200 px-4 py-3 dark:border-neutral-800">
          <img
            src="/logo.svg"
            alt="Token Guard"
            className="h-7 w-7 shrink-0"
          />
          <div>
            <h1 className="text-sm font-semibold leading-tight">
              {t("appTitle")}
            </h1>
            <p className="text-[10px] text-neutral-500">{t("appSubtitle")}</p>
          </div>
        </div>

        <nav className="flex-1 space-y-0.5 overflow-y-auto p-2">
          {tabs.map((tb) => (
            <button
              key={tb.id}
              onClick={() => setTab(tb.id)}
              className={`flex w-full items-center rounded-md px-3 py-2 text-left text-xs font-medium transition-colors ${
                tab === tb.id
                  ? "bg-emerald-500/10 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300"
                  : "text-neutral-600 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-900/60"
              }`}
            >
              {tb.label}
            </button>
          ))}
        </nav>

        <Banner licensed={licensed} />
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center justify-between border-b border-neutral-200 px-5 py-3 dark:border-neutral-800">
          <h2 className="text-sm font-semibold text-neutral-700 dark:text-neutral-300">
            {tabs.find((tb) => tb.id === tab)?.label}
          </h2>
          <div className="flex items-center gap-4">
            <div className="text-right">
              <div className="text-base font-semibold text-emerald-600 dark:text-emerald-400">
                ${spend.today.toFixed(4)}
              </div>
              <div className="text-[11px] text-neutral-500">
                {t("today")} / ${spend.budget.toFixed(2)} {t("budget")}
              </div>
            </div>
            <ThemeToggle />
            <button
              onClick={togglePause}
              className={`rounded-md px-3 py-1.5 text-xs font-medium ${
                settings?.paused
                  ? "bg-amber-500/20 text-amber-700 hover:bg-amber-500/30 dark:text-amber-300"
                  : "bg-emerald-500/20 text-emerald-700 hover:bg-emerald-500/30 dark:text-emerald-300"
              }`}
            >
              {settings?.paused ? t("paused") : t("active")}
            </button>
          </div>
        </header>

        <main className="flex-1 overflow-auto p-4">
          {tab === "dashboard" && <Dashboard proxyUrl={settings?.proxy_url} />}
          {tab === "config" && <Config onChange={() => setTick((t) => t + 1)} />}
          {tab === "settings" && (
            <SettingsTab
              settings={settings}
              licensed={licensed}
              onLicenseChange={setLicensed}
              onChanged={() => setTick((t) => t + 1)}
            />
          )}
          {tab === "docs" && <Docs onClose={() => setTab("dashboard")} />}
        </main>
      </div>
    </div>
  );
}
