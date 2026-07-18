import { useEffect, useState, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import UsageChart from "./UsageChart";
import DailyUsageChart from "./DailyUsageChart";
import Onboarding from "./Onboarding";
import Logs from "./Logs";
import { useI18n } from "../i18n";

type Log = {
  id: number;
  ts: string;
  provider: string;
  model: string;
  prompt_tokens: number;
  completion_tokens: number;
  cost: number;
  duration_ms: number;
  project_tag: string | null;
};

type LimitStatus = {
  limit: {
    id: number;
    name: string;
    metric: "money" | "tokens" | "requests" | "time_sec";
    cap: number;
    warning_threshold: number;
    action: "warn" | "block" | "pause";
    enabled: boolean;
  };
  used: number;
  ratio: number;
};

type DailyUsage = {
  day: string;
  cost: number;
  tokens: number;
  requests: number;
};

type MonthlyUsage = {
  month: string;
  cost: number;
  tokens: number;
  requests: number;
};

type ProjectTotal = {
  project_tag: string | null;
  cost: number;
  tokens: number;
  requests: number;
};

type SimpleProvider = { id: number; name: string };
type SimpleProject = { id: number; name: string };

type Range = "today" | "7d" | "30d" | "all";
type DashboardTab = "usage" | "limits" | "logs";
type Metric = "cost" | "tokens" | "requests";

export default function Dashboard({ proxyUrl }: { proxyUrl?: string }) {
  const { t } = useI18n();
  const RANGES: { id: Range; label: string }[] = [
    { id: "today", label: t("today") },
    { id: "7d", label: `7 ${t("days")}` },
    { id: "30d", label: `30 ${t("days")}` },
    { id: "all", label: t("allTime") },
  ];
  const [logs, setLogs] = useState<Log[]>([]);
  const [spend, setSpend] = useState({ today: 0, budget: 0 });
  const [range, setRange] = useState<Range>("today");
  const [chartMetric, setChartMetric] = useState<Metric>("cost");
  const [limitStatus, setLimitStatus] = useState<LimitStatus[]>([]);
  const [providerCount, setProviderCount] = useState(-1);
  const [projectCount, setProjectCount] = useState(-1);
  const [onboardingDone, setOnboardingDone] = useState(false);
  const [tab, setTab] = useState<DashboardTab>("usage");

  const [monthlyUsage, setMonthlyUsage] = useState<MonthlyUsage[]>([]);
  const [monthlyMetric, setMonthlyMetric] = useState<Metric>("cost");
  const [projectTotals, setProjectTotals] = useState<ProjectTotal[]>([]);
  const [selectedProject, setSelectedProject] = useState<string>("");

  const rangeDays = useMemo(() => {
    switch (range) {
      case "today":
        return 1;
      case "7d":
        return 7;
      case "30d":
        return 30;
      case "all":
        return 0;
    }
  }, [range]);

  const refresh = useCallback(() => {
    invoke<Log[]>("get_logs", { limit: 5000 }).then(setLogs).catch(console.error);
    invoke<{ today: number; budget: number }>("get_today_spend")
      .then(setSpend)
      .catch(console.error);
    invoke<LimitStatus[]>("get_limit_status")
      .then(setLimitStatus)
      .catch(console.error);
    invoke<{ provider: SimpleProvider }[]>("list_providers")
      .then((list) => setProviderCount(list.length))
      .catch(console.error);
    invoke<SimpleProject[]>("list_projects")
      .then((list) => setProjectCount(list.length))
      .catch(console.error);
    invoke<{ onboarding_completed: boolean }>("get_settings")
      .then((s) => setOnboardingDone(s.onboarding_completed))
      .catch(console.error);
    invoke<MonthlyUsage[]>("get_monthly_usage", { months: 12 })
      .then(setMonthlyUsage)
      .catch(console.error);
    invoke<ProjectTotal[]>("get_project_totals", { days: rangeDays })
      .then(setProjectTotals)
      .catch(console.error);
  }, [rangeDays]);

  useEffect(() => {
    refresh();
    const i = setInterval(refresh, 3000);
    return () => clearInterval(i);
  }, [refresh]);

  const shown = useMemo(() => {
    const now = Date.now();
    return logs.filter((l) => {
      if (selectedProject && l.project_tag !== selectedProject) return false;
      const t = new Date(l.ts).getTime();
      if (range === "all") return true;
      if (range === "today") {
        const logDay = new Date(l.ts).toISOString().slice(0, 10);
        const today = new Date().toISOString().slice(0, 10);
        return logDay === today;
      }
      const days = range === "7d" ? 7 : 30;
      return t >= now - days * 86_400_000;
    });
  }, [logs, range, selectedProject]);

  const totalCost = shown.reduce((a, l) => a + l.cost, 0);
  const totalTokens = shown.reduce(
    (a, l) => a + l.prompt_tokens + l.completion_tokens,
    0
  );

  const saveFile = async (defaultName: string, content: string, extension: string) => {
    const path = await save({
      defaultPath: defaultName,
      filters: [{ name: extension.toUpperCase(), extensions: [extension] }],
    });
    if (!path) return;
    try {
      await invoke("write_text_file", { path, content });
    } catch (err) {
      console.error("save failed", err);
      alert("Save failed: " + String(err));
    }
  };

  const exportCsv = async () => {
    const header =
      "timestamp,provider,model,prompt_tokens,completion_tokens,cost,duration_ms,project\n";
    const rows = shown
      .slice()
      .reverse()
      .map(
        (l) =>
          `${l.ts},${esc(l.provider)},${esc(l.model)},${l.prompt_tokens},${
            l.completion_tokens
          },${l.cost.toFixed(6)},${l.duration_ms},${esc(l.project_tag ?? "")}`
      )
      .join("\n");
    await saveFile(`tokenguard-${range}.csv`, header + rows, "csv");
  };

  const exportJson = async () =>
    await saveFile(
      `tokenguard-${range}.json`,
      JSON.stringify(shown, null, 2),
      "json"
    );

  if (providerCount === 0 && projectCount === 0 && !onboardingDone) {
    const completeOnboarding = () => {
      invoke("set_onboarding_completed")
        .then(refresh)
        .catch(console.error);
    };
    return <Onboarding onComplete={completeOnboarding} />;
  }

  const tabs: { id: DashboardTab; label: string }[] = [
    { id: "usage", label: t("usage") },
    { id: "limits", label: t("limits") },
    { id: "logs", label: t("logs") },
  ];

  return (
    <div className="space-y-4 text-neutral-900 dark:text-neutral-100">
      <div className="flex gap-1 border-b border-neutral-200 dark:border-neutral-800">
        {tabs.map((tb) => (
          <button
            key={tb.id}
            onClick={() => setTab(tb.id)}
            className={`-mb-px border-b-2 px-3 py-2 text-xs font-medium transition-colors ${
              tab === tb.id
                ? "border-emerald-500 text-neutral-900 dark:text-neutral-100"
                : "border-transparent text-neutral-500 hover:text-neutral-700 dark:hover:text-neutral-300"
            }`}
          >
            {tb.label}
          </button>
        ))}
      </div>

      {tab === "usage" && (
        <>
          <div className="grid grid-cols-3 gap-3">
            <Stat label={t("today")} value={`$${spend.today.toFixed(4)}`} accent="emerald" />
            <Stat label={t("budget")} value={`$${spend.budget.toFixed(2)}`} accent="neutral" />
            <Stat
              label={t("budgetUsed")}
              value={
                spend.budget > 0
                  ? `${((spend.today / spend.budget) * 100).toFixed(1)}%`
                  : "—"
              }
              accent="amber"
            />
          </div>

          <Forecast spend={spend} t={t} />

          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-2">
              <div className="flex gap-1">
                {RANGES.map((r) => (
                  <button
                    key={r.id}
                    onClick={() => setRange(r.id)}
                    className={`rounded-md px-2.5 py-1 text-xs font-medium ${
                      range === r.id
                        ? "bg-emerald-500/20 text-emerald-700 dark:text-emerald-300"
                        : "bg-neutral-200 text-neutral-600 hover:text-neutral-900 dark:bg-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-200"
                    }`}
                  >
                    {r.label}
                  </button>
                ))}
              </div>
              <select
                value={selectedProject}
                onChange={(e) => setSelectedProject(e.target.value)}
                className="rounded-md border border-neutral-300 bg-white px-2.5 py-1 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
              >
                <option value="">{t("allProjects")}</option>
                {projectTotals.map((pt) => (
                  <option key={pt.project_tag ?? "__untagged"} value={pt.project_tag ?? ""}>
                    {pt.project_tag ?? "(untagged)"} — ${pt.cost.toFixed(4)}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex gap-2">
              <button
                onClick={exportCsv}
                className="rounded-md bg-neutral-200 px-2.5 py-1 text-xs text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
              >
                {t("exportCsv")}
              </button>
              <button
                onClick={exportJson}
                className="rounded-md bg-neutral-200 px-2.5 py-1 text-xs text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
              >
                {t("exportJson")}
              </button>
            </div>
          </div>

          <div className="grid grid-cols-3 gap-3">
            <Stat label={`${t("cost")} (${range})`} value={`$${totalCost.toFixed(4)}`} accent="violet" />
            <Stat label={`${t("tokens")} (${range})`} value={totalTokens.toLocaleString()} accent="sky" />
            <Stat label={`${t("requests")} (${range})`} value={String(shown.length)} accent="neutral" />
          </div>

          <ProjectSpend totals={projectTotals} t={t} />

          <div className="rounded-lg border border-neutral-200 p-3 dark:border-neutral-800">
            <div className="mb-2 flex items-center justify-between">
              <h3 className="text-xs font-medium text-neutral-500 dark:text-neutral-400">
                {t("usageOverTime")}
              </h3>
              <div className="flex gap-1">
                {(["cost", "tokens", "requests"] as const).map((m) => (
                  <button
                    key={m}
                    onClick={() => setChartMetric(m)}
                    className={`rounded px-2 py-0.5 text-[10px] font-medium ${
                      chartMetric === m
                        ? "bg-emerald-500/20 text-emerald-700 dark:text-emerald-300"
                        : "bg-neutral-200 text-neutral-600 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-400 dark:hover:bg-neutral-700"
                    }`}
                  >
                    {t(m)}
                  </button>
                ))}
              </div>
            </div>
            <UsageChart logs={shown} metric={chartMetric} range={range} t={t} />
          </div>

          <div className="rounded-lg border border-neutral-200 p-3 dark:border-neutral-800">
            <div className="mb-2 flex items-center justify-between">
              <h3 className="text-xs font-medium text-neutral-500 dark:text-neutral-400">
                {t("monthlyUsage")}
              </h3>
              <div className="flex gap-1">
                {(["cost", "tokens", "requests"] as const).map((m) => (
                  <button
                    key={m}
                    onClick={() => setMonthlyMetric(m)}
                    className={`rounded px-2 py-0.5 text-[10px] font-medium ${
                      monthlyMetric === m
                        ? "bg-emerald-500/20 text-emerald-700 dark:text-emerald-300"
                        : "bg-neutral-200 text-neutral-600 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-400 dark:hover:bg-neutral-700"
                    }`}
                  >
                    {t(m)}
                  </button>
                ))}
              </div>
            </div>
            <DailyUsageChart
              data={monthlyUsage.map((u) => ({
                day: u.month,
                cost: u.cost,
                tokens: u.tokens,
                requests: u.requests,
              }))}
              metric={monthlyMetric}
              t={t}
            />
          </div>

          <div className="overflow-hidden rounded-lg border border-neutral-200 dark:border-neutral-800">
            <table className="w-full text-left text-xs">
              <thead className="bg-neutral-100 text-neutral-600 dark:bg-neutral-900 dark:text-neutral-400">
                <tr>
                  <th className="px-3 py-2 font-medium">{t("time")}</th>
                  <th className="px-3 py-2 font-medium">{t("provider")}</th>
                  <th className="px-3 py-2 font-medium">{t("model")}</th>
                  <th className="px-3 py-2 text-right font-medium">{t("prompt")}</th>
                  <th className="px-3 py-2 text-right font-medium">{t("completion")}</th>
                  <th className="px-3 py-2 text-right font-medium">{t("cost")}</th>
                  <th className="px-3 py-2 font-medium">{t("project")}</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-neutral-200 dark:divide-neutral-800">
                {shown.length === 0 && (
                  <tr>
                    <td colSpan={7} className="px-3 py-6 text-center text-neutral-600">
                      {t("noRequestsInRange")}{" "}
                      <code className="text-neutral-600 dark:text-neutral-400">
                        {proxyUrl ?? "http://<ip>:3742"}
                      </code>{" "}
                      {t("andSendRequest")}
                    </td>
                  </tr>
                )}
                {shown.map((l) => (
                  <tr key={l.id} className="hover:bg-neutral-100 dark:hover:bg-neutral-900/60">
                    <td className="px-3 py-1.5 text-neutral-500 dark:text-neutral-400">
                      {new Date(l.ts).toLocaleString()}
                    </td>
                    <td className="px-3 py-1.5 text-neutral-700 dark:text-neutral-300">{l.provider}</td>
                    <td className="px-3 py-1.5 font-mono text-neutral-500 dark:text-neutral-400">
                      {l.model}
                    </td>
                    <td className="px-3 py-1.5 text-right text-neutral-500 dark:text-neutral-400">
                      {l.prompt_tokens.toLocaleString()}
                    </td>
                    <td className="px-3 py-1.5 text-right text-neutral-500 dark:text-neutral-400">
                      {l.completion_tokens.toLocaleString()}
                    </td>
                    <td className="px-3 py-1.5 text-right font-medium text-emerald-600 dark:text-emerald-400">
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
        </>
      )}

      {tab === "limits" && (
        <div className="space-y-4">
          {limitStatus.length === 0 && (
            <p className="text-xs text-neutral-500">{t("noActiveLimits")}</p>
          )}
          {limitStatus.length > 0 && (
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
              {limitStatus.map((s) => {
                const ratio = Math.min(s.ratio, 1);
                const accent =
                  ratio >= 1
                    ? "red"
                    : ratio >= s.limit.warning_threshold
                    ? "amber"
                    : "emerald";
                return (
                  <div
                    key={s.limit.id}
                    className="rounded-lg border border-neutral-200 bg-white p-3 dark:border-neutral-800 dark:bg-neutral-900/40"
                  >
                    <div className="flex items-center justify-between">
                      <span className="text-xs font-medium text-neutral-800 dark:text-neutral-200">
                        {s.limit.name}
                      </span>
                      <span
                        className={`rounded px-1.5 py-0.5 text-[10px] ${
                          s.limit.action === "block"
                            ? "bg-red-500/20 text-red-700 dark:text-red-300"
                            : s.limit.action === "pause"
                            ? "bg-amber-500/20 text-amber-700 dark:text-amber-300"
                            : "bg-sky-500/20 text-sky-700 dark:text-sky-300"
                        }`}
                      >
                        {s.limit.action}
                      </span>
                    </div>
                    <div className="mt-1 text-sm font-semibold text-neutral-900 dark:text-neutral-200">
                      {formatLimitValue(s.limit.metric, s.used)} /{" "}
                      {formatLimitValue(s.limit.metric, s.limit.cap)}
                    </div>
                    <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-neutral-800">
                      <div
                        className={`h-full ${
                          accent === "red"
                            ? "bg-red-500"
                            : accent === "amber"
                            ? "bg-amber-500"
                            : "bg-emerald-500"
                        }`}
                        style={{ width: `${ratio * 100}%` }}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {tab === "logs" && <Logs />}
    </div>
  );
}

function esc(s: string): string {
  if (s.includes(",") || s.includes('"') || s.includes("\n"))
    return `"${s.replace(/"/g, '""')}"`;
  return s;
}

function formatLimitValue(metric: LimitStatus["limit"]["metric"], value: number): string {
  if (metric === "money") return `$${value.toFixed(2)}`;
  if (metric === "time_sec") {
    const hours = value / 3600;
    if (hours >= 1) return `${hours.toFixed(1)}h`;
    const m = Math.floor(value / 60);
    if (m > 0) return `${m}m`;
    return `${Math.round(value)}s`;
  }
  return Math.round(value).toLocaleString();
}

function Forecast({
  spend,
  t,
}: {
  spend: { today: number; budget: number };
  t: (
    key: keyof typeof import("../i18n").translations.en,
    vars?: Record<string, string | number>
  ) => string;
}) {
  const text = useMemo(() => {
    if (spend.budget <= 0) return null;
    if (spend.today <= 0) return t("forecastNoSpend");
    if (spend.today >= spend.budget) return t("forecastOverBudget");
    const now = new Date();
    const midnightUtc = Date.UTC(
      now.getUTCFullYear(),
      now.getUTCMonth(),
      now.getUTCDate()
    );
    const hoursElapsed = (Date.now() - midnightUtc) / 3600_000;
    if (hoursElapsed <= 0) return t("forecastNoSpend");
    const rate = spend.today / hoursElapsed;
    const hoursToHit = (spend.budget - spend.today) / rate;
    return t("forecastHitBudget", { hours: hoursToHit.toFixed(1) });
  }, [spend, t]);

  if (!text) return null;
  return (
    <div className="rounded-lg border border-emerald-200 bg-emerald-50 px-4 py-2 text-xs text-emerald-800 dark:border-emerald-900 dark:bg-emerald-900/20 dark:text-emerald-300">
      <span className="font-semibold">{t("forecast")}:</span> {text}
    </div>
  );
}

function ProjectSpend({
  totals,
  t,
}: {
  totals: ProjectTotal[];
  t: (
    key: keyof typeof import("../i18n").translations.en,
    vars?: Record<string, string | number>
  ) => string;
}) {
  if (totals.length === 0) return null;
  const max = Math.max(...totals.map((p) => p.cost), 0.0001);
  return (
    <div className="rounded-lg border border-neutral-200 p-3 dark:border-neutral-800">
      <h3 className="mb-2 text-xs font-medium text-neutral-500 dark:text-neutral-400">
        {t("topProjects")}
      </h3>
      <div className="space-y-2">
        {totals.slice(0, 8).map((pt) => (
          <div key={pt.project_tag ?? "__untagged"} className="flex items-center gap-3">
            <div className="w-24 truncate text-xs text-neutral-700 dark:text-neutral-300">
              {pt.project_tag ?? "(untagged)"}
            </div>
            <div className="flex-1 overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-800">
              <div
                className="h-2 rounded-full bg-emerald-500"
                style={{ width: `${Math.min((pt.cost / max) * 100, 100)}%` }}
              />
            </div>
            <div className="w-20 text-right text-xs text-neutral-500">
              ${pt.cost.toFixed(4)}
            </div>
          </div>
        ))}
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
    emerald: "text-emerald-600 dark:text-emerald-400",
    neutral: "text-neutral-700 dark:text-neutral-200",
    sky: "text-sky-600 dark:text-sky-400",
    violet: "text-violet-600 dark:text-violet-400",
    amber: "text-amber-600 dark:text-amber-400",
  };
  return (
    <div className="rounded-lg border border-neutral-200 bg-white px-4 py-3 dark:border-neutral-800 dark:bg-neutral-900/40">
      <div className="text-[11px] uppercase tracking-wide text-neutral-500">
        {label}
      </div>
      <div className={`mt-1 text-xl font-semibold ${colors[accent]}`}>
        {value}
      </div>
    </div>
  );
}
