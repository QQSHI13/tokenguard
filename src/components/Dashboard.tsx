import { useEffect, useState, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";

type Log = {
  id: number;
  ts: string;
  provider: string;
  model: string;
  prompt_tokens: number;
  completion_tokens: number;
  cost: number;
  project_tag: string | null;
};

type Range = "today" | "7d" | "30d" | "all";
const RANGES: { id: Range; label: string }[] = [
  { id: "today", label: "Today" },
  { id: "7d", label: "7 Days" },
  { id: "30d", label: "30 Days" },
  { id: "all", label: "All Time" },
];

export default function Dashboard() {
  const [logs, setLogs] = useState<Log[]>([]);
  const [spend, setSpend] = useState({ today: 0, budget: 0 });
  const [range, setRange] = useState<Range>("today");

  const refresh = useCallback(() => {
    invoke<Log[]>("get_logs", { limit: 5000 }).then(setLogs).catch(console.error);
    invoke<{ today: number; budget: number }>("get_today_spend")
      .then(setSpend)
      .catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
    const i = setInterval(refresh, 3000);
    return () => clearInterval(i);
  }, [refresh]);

  const shown = useMemo(() => {
    const now = Date.now();
    return logs.filter((l) => {
      const t = new Date(l.ts).getTime();
      if (range === "all") return true;
      if (range === "today")
        return new Date(l.ts).toDateString() === new Date().toDateString();
      const days = range === "7d" ? 7 : 30;
      return t >= now - days * 86_400_000;
    });
  }, [logs, range]);

  const totalCost = shown.reduce((a, l) => a + l.cost, 0);
  const totalTokens = shown.reduce(
    (a, l) => a + l.prompt_tokens + l.completion_tokens,
    0
  );

  const download = (name: string, content: string, type: string) => {
    const blob = new Blob([content], { type });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = name;
    a.click();
    URL.revokeObjectURL(url);
  };

  const exportCsv = () => {
    const header =
      "timestamp,provider,model,prompt_tokens,completion_tokens,cost,project\n";
    const rows = shown
      .slice()
      .reverse()
      .map(
        (l) =>
          `${l.ts},${esc(l.provider)},${esc(l.model)},${l.prompt_tokens},${
            l.completion_tokens
          },${l.cost.toFixed(6)},${esc(l.project_tag ?? "")}`
      )
      .join("\n");
    download(
      `tokenguard-${range}.csv`,
      header + rows,
      "text/csv"
    );
  };

  const exportJson = () =>
    download(
      `tokenguard-${range}.json`,
      JSON.stringify(shown, null, 2),
      "application/json"
    );

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-3 gap-3">
        <Stat label="Today" value={`$${spend.today.toFixed(4)}`} accent="emerald" />
        <Stat label="Budget" value={`$${spend.budget.toFixed(2)}`} accent="neutral" />
        <Stat
          label="Budget used"
          value={
            spend.budget > 0
              ? `${((spend.today / spend.budget) * 100).toFixed(1)}%`
              : "—"
          }
          accent="amber"
        />
      </div>

      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex gap-1">
          {RANGES.map((r) => (
            <button
              key={r.id}
              onClick={() => setRange(r.id)}
              className={`rounded-md px-2.5 py-1 text-xs font-medium ${
                range === r.id
                  ? "bg-emerald-500/20 text-emerald-300"
                  : "bg-neutral-800 text-neutral-400 hover:text-neutral-200"
              }`}
            >
              {r.label}
            </button>
          ))}
        </div>
        <div className="flex gap-2">
          <button
            onClick={exportCsv}
            className="rounded-md bg-neutral-800 px-2.5 py-1 text-xs text-neutral-200 hover:bg-neutral-700"
          >
            Export CSV
          </button>
          <button
            onClick={exportJson}
            className="rounded-md bg-neutral-800 px-2.5 py-1 text-xs text-neutral-200 hover:bg-neutral-700"
          >
            Export JSON
          </button>
        </div>
      </div>

      <div className="grid grid-cols-3 gap-3">
        <Stat label={`Cost (${range})`} value={`$${totalCost.toFixed(4)}`} accent="violet" />
        <Stat label={`Tokens (${range})`} value={totalTokens.toLocaleString()} accent="sky" />
        <Stat label={`Requests (${range})`} value={String(shown.length)} accent="neutral" />
      </div>

      <div className="overflow-hidden rounded-lg border border-neutral-800">
        <table className="w-full text-left text-xs">
          <thead className="bg-neutral-900 text-neutral-400">
            <tr>
              <th className="px-3 py-2 font-medium">Time</th>
              <th className="px-3 py-2 font-medium">Provider</th>
              <th className="px-3 py-2 font-medium">Model</th>
              <th className="px-3 py-2 text-right font-medium">Prompt</th>
              <th className="px-3 py-2 text-right font-medium">Completion</th>
              <th className="px-3 py-2 text-right font-medium">Cost</th>
              <th className="px-3 py-2 font-medium">Project</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-neutral-800">
            {shown.length === 0 && (
              <tr>
                <td colSpan={7} className="px-3 py-6 text-center text-neutral-600">
                  No requests in this range. Point your LLM client at{" "}
                  <code className="text-neutral-400">http://localhost:3742</code>{" "}
                  and send a request.
                </td>
              </tr>
            )}
            {shown.map((l) => (
              <tr key={l.id} className="hover:bg-neutral-900/60">
                <td className="px-3 py-1.5 text-neutral-400">
                  {new Date(l.ts).toLocaleString()}
                </td>
                <td className="px-3 py-1.5 text-neutral-300">{l.provider}</td>
                <td className="px-3 py-1.5 font-mono text-neutral-400">
                  {l.model}
                </td>
                <td className="px-3 py-1.5 text-right text-neutral-400">
                  {l.prompt_tokens.toLocaleString()}
                </td>
                <td className="px-3 py-1.5 text-right text-neutral-400">
                  {l.completion_tokens.toLocaleString()}
                </td>
                <td className="px-3 py-1.5 text-right font-medium text-emerald-400">
                  ${l.cost.toFixed(6)}
                </td>
                <td className="px-3 py-1.5 text-neutral-500">
                  {l.project_tag ?? "—"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function esc(s: string): string {
  if (s.includes(",") || s.includes('"') || s.includes("\n"))
    return `"${s.replace(/"/g, '""')}"`;
  return s;
}

function Stat({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent: "emerald" | "neutral" | "sky" | "violet" | "amber";
}) {
  const colors: Record<string, string> = {
    emerald: "text-emerald-400",
    neutral: "text-neutral-200",
    sky: "text-sky-400",
    violet: "text-violet-400",
    amber: "text-amber-400",
  };
  return (
    <div className="rounded-lg border border-neutral-800 bg-neutral-900/40 px-4 py-3">
      <div className="text-[11px] uppercase tracking-wide text-neutral-500">
        {label}
      </div>
      <div className={`mt-1 text-xl font-semibold ${colors[accent]}`}>
        {value}
      </div>
    </div>
  );
}
