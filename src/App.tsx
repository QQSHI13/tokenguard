import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import Providers from "./components/Providers";
import Dashboard from "./components/Dashboard";
import SettingsTab from "./components/Settings";

type Settings = {
  port: number;
  budget: number;
  paused: boolean;
  proxy_url: string;
  provider_count: number;
};
type Spend = { today: number; budget: number };

type Tab = "dashboard" | "providers" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("dashboard");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [spend, setSpend] = useState<Spend>({ today: 0, budget: 0 });
  const [tick, setTick] = useState(0);

  const refresh = useCallback(() => {
    invoke<Settings>("get_settings").then(setSettings).catch(console.error);
    invoke<Spend>("get_today_spend").then(setSpend).catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
    const i = setInterval(refresh, 4000);
    return () => clearInterval(i);
  }, [refresh, tick]);

  const togglePause = async () => {
    const paused = await invoke<boolean>("pause_resume");
    setSettings((s) => (s ? { ...s, paused } : s));
  };

  const tabs: { id: Tab; label: string }[] = [
    { id: "dashboard", label: "Dashboard" },
    { id: "providers", label: "Providers" },
    { id: "settings", label: "Settings" },
  ];

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-neutral-800 px-5 py-3">
        <div className="flex items-center gap-3">
          <span className="text-lg">🛡️</span>
          <div>
            <h1 className="text-sm font-semibold tracking-wide text-neutral-100">
              Token Guard
            </h1>
            <p className="text-[11px] text-neutral-500">
              The local LLM gateway — nothing leaves your machine.
            </p>
          </div>
        </div>
        <div className="flex items-center gap-4">
          <div className="text-right">
            <div className="text-base font-semibold text-emerald-400">
              ${spend.today.toFixed(4)}
            </div>
            <div className="text-[11px] text-neutral-500">
              today / ${spend.budget.toFixed(2)} budget
            </div>
          </div>
          <button
            onClick={togglePause}
            className={`rounded-md px-3 py-1.5 text-xs font-medium ${
              settings?.paused
                ? "bg-amber-500/20 text-amber-300 hover:bg-amber-500/30"
                : "bg-emerald-500/20 text-emerald-300 hover:bg-emerald-500/30"
            }`}
          >
            {settings?.paused ? "Paused" : "Active"}
          </button>
        </div>
      </header>

      <nav className="flex gap-1 border-b border-neutral-800 px-3">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`-mb-px border-b-2 px-3 py-2 text-xs font-medium transition-colors ${
              tab === t.id
                ? "border-emerald-500 text-neutral-100"
                : "border-transparent text-neutral-500 hover:text-neutral-300"
            }`}
          >
            {t.label}
          </button>
        ))}
      </nav>

      <main className="flex-1 overflow-auto p-4">
        {tab === "dashboard" && <Dashboard />}
        {tab === "providers" && (
          <Providers onChange={() => setTick((t) => t + 1)} />
        )}
        {tab === "settings" && (
          <SettingsTab
            settings={settings}
            onChanged={() => setTick((t) => t + 1)}
          />
        )}
      </main>
    </div>
  );
}
