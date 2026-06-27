import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

type Settings = {
  port: number;
  budget: number;
  paused: boolean;
  proxy_url: string;
  provider_count: number;
  accurate_streaming: boolean;
} | null;

export default function SettingsTab({
  settings,
  onChanged,
}: {
  settings: Settings;
  onChanged: () => void;
}) {
  const [budget, setBudget] = useState(String(settings?.budget ?? 0));
  const [port, setPort] = useState(String(settings?.port ?? 3742));
  const [accurate, setAccurate] = useState(settings?.accurate_streaming ?? true);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (settings) setAccurate(settings.accurate_streaming);
  }, [settings]);

  const saveBudget = async () => {
    await invoke("set_budget", { budget: Number(budget) || 0 });
    onChanged();
  };
  const savePort = async () => {
    await invoke("set_port", { port: Number(port) || 3742 });
    onChanged();
  };
  const toggleAccurate = async (v: boolean) => {
    setAccurate(v);
    await invoke("set_accurate_streaming", { enabled: v });
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
    <div className="max-w-xl space-y-6">
      <section className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4">
        <h2 className="text-sm font-semibold text-neutral-200">Proxy endpoint</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          Point your LLM client's base URL here. Token Guard routes by the{" "}
          <code className="text-neutral-400">model</code> field to the matching
          provider. Loopback-only — not reachable from the network.
        </p>
        <div className="mt-3 flex items-center gap-2">
          <code className="flex-1 truncate rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2 text-sm text-emerald-300">
            {settings?.proxy_url ?? "http://localhost:3742"}
          </code>
          <button
            onClick={copy}
            className="rounded-md bg-neutral-800 px-3 py-2 text-xs text-neutral-200 hover:bg-neutral-700"
          >
            {copied ? "Copied!" : "Copy"}
          </button>
        </div>
        <p className="mt-2 text-[11px] text-neutral-600">
          Example:{" "}
          <code className="text-neutral-400">
            OPENAI_BASE_URL={settings?.proxy_url}
          </code>
        </p>
      </section>

      <section className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4">
        <h2 className="text-sm font-semibold text-neutral-200">Budget</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          Tray icon turns yellow at 80% and red at 100%.
        </p>
        <div className="mt-3 flex items-center gap-2">
          <span className="text-xs text-neutral-500">$</span>
          <input
            type="number"
            step="0.5"
            value={budget}
            onChange={(e) => setBudget(e.target.value)}
            className="w-32 rounded-md border border-neutral-700 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-200"
          />
          <button
            onClick={saveBudget}
            className="rounded-md bg-emerald-500/20 px-3 py-1.5 text-xs text-emerald-300 hover:bg-emerald-500/30"
          >
            Save
          </button>
        </div>
      </section>

      <section className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4">
        <h2 className="text-sm font-semibold text-neutral-200">Port</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          Applied on next launch — the listener is bound at startup.
        </p>
        <div className="mt-3 flex items-center gap-2">
          <input
            type="number"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            className="w-32 rounded-md border border-neutral-700 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-200"
          />
          <button
            onClick={savePort}
            className="rounded-md bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:bg-neutral-700"
          >
            Save
          </button>
        </div>
      </section>

      <section className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4">
        <h2 className="text-sm font-semibold text-neutral-200">Streaming accuracy</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          Inject <code className="text-neutral-400">stream_options.include_usage</code> into
          OpenAI streaming requests so token counts are exact. This is the one
          documented request-side exception to “read-only”. Turn off for strict
          read-only (OpenAI stream counts may be 0).
        </p>
        <label className="mt-3 flex items-center gap-2 text-xs text-neutral-300">
          <input
            type="checkbox"
            checked={accurate}
            onChange={(e) => toggleAccurate(e.target.checked)}
          />
          Accurate OpenAI streaming counts (recommended)
        </label>
      </section>

      <section className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4">
        <h2 className="text-sm font-semibold text-neutral-200">Keychain</h2>
        <p className="mt-1 text-[11px] text-neutral-500">
          Diagnose API-key storage. Runs a write→read→new-entry-read probe against
          the OS keychain and reports which step fails.
        </p>
        <button
          onClick={runSelftest}
          className="mt-3 rounded-md bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:bg-neutral-700"
        >
          Test keychain
        </button>
      </section>

      <section className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4 text-[11px] leading-relaxed text-neutral-500">
        <h2 className="mb-2 text-sm font-semibold text-neutral-300">
          Privacy &amp; security
        </h2>
        <ul className="list-inside list-disc space-y-1">
          <li>No cloud, no account, no telemetry. Only your provider calls + Store license check leave the machine.</li>
          <li>API keys live in the OS keychain — never on disk, never in your code.</li>
          <li>The proxy binds 127.0.0.1 only. Unreachable from the network.</li>
          <li>Read-only: responses are never modified. OpenAI streaming requests get a documented <code>stream_options.include_usage</code> injection (opt-out coming) so token counts are accurate.</li>
        </ul>
      </section>
    </div>
  );
}
