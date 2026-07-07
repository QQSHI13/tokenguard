import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import Providers from "./components/Providers";
import Dashboard from "./components/Dashboard";
import SettingsTab from "./components/Settings";
import Projects from "./components/Projects";
import Limits from "./components/Limits";
import Logs from "./components/Logs";
import Docs from "./components/Docs";
import ThemeToggle, { initTheme } from "./components/ThemeToggle";
import Banner from "./components/Banner";
import { useI18n } from "./i18n";
import {
  isLicensed,
  validateStoredKey,
  getLicenseKey,
} from "./utils/license";
import { runAutoUpdate } from "./utils/updater";

type Settings = {
  port: number;
  budget: number;
  paused: boolean;
  proxy_url: string;
  provider_count: number;
  log_bodies: boolean;
};
type Spend = { today: number; budget: number };

type Tab = "dashboard" | "limits" | "providers" | "projects" | "settings" | "docs" | "logs";

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
    getLicenseKey().then((key) => {
      if (!key) return;
      validateStoredKey().then((valid) => {
        if (!valid) setLicensed(false);
      });
    });
  }, []);

  useEffect(() => {
    runAutoUpdate();
    const i = setInterval(runAutoUpdate, 1000 * 60 * 60 * 4); // every 4 hours
    return () => clearInterval(i);
  }, []);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    listen<string>("set_tab", (event) => {
      const t = event.payload as Tab;
      if (
        ["dashboard", "limits", "providers", "projects", "settings", "docs", "logs"].includes(t)
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
    { id: "limits", label: t("limits") },
    { id: "providers", label: t("providers") },
    { id: "projects", label: t("projects") },
    { id: "logs", label: t("logs") },
    { id: "settings", label: t("settings") },
    { id: "docs", label: t("docs") },
  ];

  return (
    <div className="flex h-full flex-col bg-white text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
      <header className="flex items-center justify-between border-b border-neutral-200 px-5 py-3 dark:border-neutral-800">
        <div className="flex items-center gap-3">
          <span className="text-lg">🛡️</span>
          <div>
            <h1 className="text-sm font-semibold tracking-wide">
              {t("appTitle")}
            </h1>
            <p className="text-[11px] text-neutral-500">{t("appSubtitle")}</p>
          </div>
        </div>
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

      <nav className="flex gap-1 overflow-x-auto border-b border-neutral-200 px-3 dark:border-neutral-800">
        {tabs.map((tb) => (
          <button
            key={tb.id}
            onClick={() => setTab(tb.id)}
            className={`-mb-px shrink-0 border-b-2 px-3 py-2 text-xs font-medium transition-colors ${
              tab === tb.id
                ? "border-emerald-500 text-neutral-900 dark:text-neutral-100"
                : "border-transparent text-neutral-500 hover:text-neutral-700 dark:hover:text-neutral-300"
            }`}
          >
            {tb.label}
          </button>
        ))}
      </nav>

      <main className="flex-1 overflow-auto p-4">
        {tab === "dashboard" && <Dashboard />}
        {tab === "limits" && <Limits onChange={() => setTick((t) => t + 1)} />}
        {tab === "providers" && (
          <Providers onChange={() => setTick((t) => t + 1)} />
        )}
        {tab === "projects" && (
          <Projects onChange={() => setTick((t) => t + 1)} />
        )}
        {tab === "logs" && <Logs />}
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
      <Banner
        licensed={licensed}
        onLicenseChange={setLicensed}
      />
    </div>
  );
}
