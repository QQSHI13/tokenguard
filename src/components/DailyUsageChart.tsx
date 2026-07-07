import { useMemo } from "react";
import type { translations } from "../i18n";

type Daily = {
  day: string;
  cost: number;
  tokens: number;
  requests: number;
};

type Metric = "cost" | "tokens" | "requests";

export default function DailyUsageChart({
  data,
  metric,
  t,
}: {
  data: Daily[];
  metric: Metric;
  t: (key: keyof typeof translations.en) => string;
}) {
  const chartData = useMemo(() => {
    if (data.length === 0) return [];
    return data.map((d) => ({
      label: d.day,
      value:
        metric === "cost"
          ? d.cost
          : metric === "tokens"
          ? d.tokens
          : d.requests,
    }));
  }, [data, metric]);

  if (chartData.length === 0) {
    return (
      <div className="flex h-40 items-center justify-center rounded-lg border border-neutral-200 text-xs text-neutral-500 dark:border-neutral-800">
        {t("noDataToChart")}
      </div>
    );
  }

  const max = Math.max(...chartData.map((d) => d.value), 0.0001);
  const width = 600;
  const height = 200;
  const pad = { top: 10, right: 10, bottom: 50, left: 50 };
  const chartW = width - pad.left - pad.right;
  const chartH = height - pad.top - pad.bottom;
  const barW = (chartW / chartData.length) * 0.7;
  const gap = (chartW / chartData.length) * 0.3;

  return (
    <div className="overflow-x-auto">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        className="h-48 w-full min-w-[400px]"
        preserveAspectRatio="xMidYMid meet"
      >
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
        {chartData.map((d, i) => {
          const x = pad.left + i * (barW + gap) + gap / 2;
          const barH = (d.value / max) * chartH;
          const y = height - pad.bottom - barH;
          return (
            <g key={d.label}>
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
                {d.label.slice(5)}
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
