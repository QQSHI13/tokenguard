import { useEffect, useState, useCallback } from "react";
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

export default function Dashboard() {
  const [logs, setLogs] = useState<Log[]>([]);
  const [spend, setSpend] = useState({ today: 0, budget: 0 });

  const refresh = useCallback(() => {
    invoke<Log[]>("get_logs", { limit: 200 }).then(setLogs).catch(console.error);
    invoke<{ today: number; budget: number }>("get_today_spend")
      .then(setSpend)
      .catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
    const i = setInterval(refresh, 3000);
    return () => clearInterval(i);
  }, [refresh]);

  const totalTokens = logs.reduce(
    (a, l) => a + l.prompt_tokens + l.completion_tokens,
    0
  );

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-3 gap-3">
        <Stat label="Today" value={`$${spend.today.toFixed(4)}`} accent="emerald" />
        <Stat label="Budget" value={`$${spend.budget.toFixed(2)}`} accent="neutral" />
        <Stat label="Requests" value={String(logs.length)} accent="sky" />
      </div>
      <div className="grid grid-cols-2 gap-3">
        <Stat label="Total tokens (last 200)" value={totalTokens.toLocaleString()} accent="violet" />
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
            </tr>
          </thead>
          <tbody className="divide-y divide-neutral-800">
            {logs.length === 0 && (
              <tr>
                <td colSpan={6} className="px-3 py-6 text-center text-neutral-600">
                  No requests yet. Point your LLM client at{" "}
                  <code className="text-neutral-400">http://localhost:3742</code>{" "}
                  and send a request.
                </td>
              </tr>
            )}
            {logs.map((l) => (
              <tr key={l.id} className="hover:bg-neutral-900/60">
                <td className="px-3 py-1.5 text-neutral-400">
                  {new Date(l.ts).toLocaleTimeString()}
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
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
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
