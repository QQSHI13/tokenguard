import { useMemo } from "react";
import type { translations } from "../i18n";

type Log = {
  ts: string;
  cost: number;
  prompt_tokens: number;
  completion_tokens: number;
};

type Metric = "cost" | "tokens" | "requests";

export default function UsageChart({
  logs,
  metric,
  range,
  t,
}: {
  logs: Log[];
  metric: Metric;
  range: "today" | "7d" | "30d" | "all";
  t: (key: keyof typeof translations.en) => string;
}) {
  const data = useMemo(() => {
    if (logs.length === 0) return [];
    const now = Date.now();
    let bucketMs: number;
    let cutoff: number;
    let labelFmt: (d: Date) => string;

    switch (range) {
      case "today":
        bucketMs = 60 * 60 * 1000; // 1 hour
        cutoff = now - 24 * 60 * 60 * 1000;
        labelFmt = (d) =>
          d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
        break;
      case "7d":
        bucketMs = 24 * 60 * 60 * 1000; // 1 day
        cutoff = now - 7 * 24 * 60 * 60 * 1000;
        labelFmt = (d) => d.toLocaleDateString([], { weekday: "short" });
        break;
      case "30d":
        bucketMs = 24 * 60 * 60 * 1000;
        cutoff = now - 30 * 24 * 60 * 60 * 1000;
        labelFmt = (d) => d.toLocaleDateString([], { month: "short", day: "numeric" });
        break;
      default:
        bucketMs = 24 * 60 * 60 * 1000;
        cutoff = Math.min(...logs.map((l) => new Date(l.ts).getTime()));
        labelFmt = (d) => d.toLocaleDateString([], { month: "short", day: "numeric" });
    }

    const buckets = new Map<number, number>();
    for (const l of logs) {
      const t = new Date(l.ts).getTime();
      if (t < cutoff) continue;
      const bucket = Math.floor(t / bucketMs) * bucketMs;
      const value =
        metric === "cost"
          ? l.cost
          : metric === "tokens"
          ? l.prompt_tokens + l.completion_tokens
          : 1;
      buckets.set(bucket, (buckets.get(bucket) || 0) + value);
    }

    const sorted = Array.from(buckets.entries()).sort((a, b) => a[0] - b[0]);
    if (sorted.length === 0) return [];

    // Fill empty buckets so the x-axis is continuous.
    const first = Math.floor(cutoff / bucketMs) * bucketMs;
    const last = sorted[sorted.length - 1][0];
    const filled: { t: number; label: string; value: number }[] = [];
    for (let t = first; t <= last; t += bucketMs) {
      filled.push({
        t,
        label: labelFmt(new Date(t)),
        value: buckets.get(t) || 0,
      });
    }
    return filled;
  }, [logs, metric, range]);

  if (data.length === 0) {
    return (
      <div className="flex h-40 items-center justify-center rounded-lg border border-neutral-200 text-xs text-neutral-500 dark:border-neutral-800">
        {t("noDataToChart")}
      </div>
    );
  }

  const max = Math.max(...data.map((d) => d.value), 0.0001);
  const width = 600;
  const height = 200;
  const pad = { top: 10, right: 10, bottom: 40, left: 50 };
  const chartW = width - pad.left - pad.right;
  const chartH = height - pad.top - pad.bottom;
  const barW = chartW / data.length * 0.7;
  const gap = chartW / data.length * 0.3;

  return (
    <div className="overflow-x-auto">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        className="h-48 w-full min-w-[400px]"
        preserveAspectRatio="xMidYMid meet"
      >
        {/* axes */}
        <line
          x1={pad.left}
          y1={pad.top}
          x2={pad.left}
          y2={height - pad.bottom}
          className="stroke-neutral-300 dark:stroke-neutral-700"
          strokeWidth={1}
        />
        <line
          x1={pad.left}
          y1={height - pad.bottom}
          x2={width - pad.right}
          y2={height - pad.bottom}
          className="stroke-neutral-300 dark:stroke-neutral-700"
          strokeWidth={1}
        />
        {/* y-axis ticks */}
        {[0, 0.25, 0.5, 0.75, 1].map((p) => {
          const y = height - pad.bottom - p * chartH;
          return (
            <g key={p}>
              <line
                x1={pad.left - 4}
                y1={y}
                x2={pad.left}
                y2={y}
                className="stroke-neutral-300 dark:stroke-neutral-700"
                strokeWidth={1}
              />
              <text
                x={pad.left - 8}
                y={y + 3}
                textAnchor="end"
                className="fill-neutral-500 text-[10px]"
              >
                {formatTick(metric, p * max)}
              </text>
            </g>
          );
        })}
        {/* bars */}
        {data.map((d, i) => {
          const x = pad.left + i * (barW + gap) + gap / 2;
          const barH = (d.value / max) * chartH;
          const y = height - pad.bottom - barH;
          return (
            <g key={d.t}>
              <rect
                x={x}
                y={y}
                width={Math.max(barW, 1)}
                height={barH}
                rx={2}
                className="fill-emerald-500/70"
              />
              <text
                x={x + barW / 2}
                y={height - pad.bottom + 14}
                textAnchor="middle"
                className="fill-neutral-500 text-[9px]"
              >
                {d.label}
              </text>
              <title>
                {d.label}: {formatValue(metric, d.value)}
              </title>
            </g>
          );
        })}
      </svg>
    </div>
  );
}

function formatTick(metric: Metric, value: number): string {
  if (metric === "cost") {
    if (value >= 1) return `$${value.toFixed(1)}`;
    return `$${value.toFixed(2)}`;
  }
  if (value >= 1000) return `${(value / 1000).toFixed(1)}k`;
  return Math.round(value).toString();
}

function formatValue(metric: Metric, value: number): string {
  if (metric === "cost") return `$${value.toFixed(4)}`;
  if (metric === "tokens") return `${Math.round(value).toLocaleString()} tokens`;
  return `${Math.round(value)} requests`;
}
