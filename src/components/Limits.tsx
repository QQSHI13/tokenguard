import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

type LimitMetric =
  | "money"
  | "tokens"
  | "requests"
  | "time_sec"
  | "requests_per_minute"
  | "tokens_per_minute";
type LimitPeriod = "once" | "hourly" | "daily" | "weekly" | "monthly" | { custom_sec: number };
type LimitScope = "global" | "provider" | "project";
type LimitAction = "warn" | "block" | "pause";

type Limit = {
  id: number;
  name: string;
  metric: LimitMetric;
  period: LimitPeriod;
  cap: number;
  warning_threshold: number;
  scope: LimitScope;
  scope_id: number | null;
  action: LimitAction;
  enabled: boolean;
};

type LimitInput = Omit<Limit, "id">;

type LimitStatus = {
  limit: Limit;
  used: number;
  ratio: number;
};

type LimitPreset = {
  name: string;
  input: LimitInput;
};

type SimpleProvider = { id: number; name: string };
type SimpleProject = { id: number; name: string };



function formatMetricValue(metric: LimitMetric, value: number): string {
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

function periodLabel(period: LimitPeriod, t: (key: keyof typeof import("../i18n").translations.en) => string): string {
  if (typeof period === "string") {
    return t(period);
  }
  const sec = period.custom_sec;
  if (sec < 60) return `${sec}${t("secondsShort")}`;
  if (sec < 3600) return `${Math.round(sec / 60)}${t("minutesShort")}`;
  if (sec < 86400) return `${Math.round(sec / 3600)}${t("hoursShort")}`;
  return `${Math.round(sec / 86400)}${t("daysShort")}`;
}

function displayCap(metric: LimitMetric, cap: number): string | number {
  if (metric === "time_sec") return cap / 3600;
  return cap || "";
}

function storeCap(metric: LimitMetric, value: number): number {
  if (metric === "time_sec") return value * 3600;
  return value;
}

function blankLimit(): LimitInput {
  return {
    name: "",
    metric: "money",
    period: "daily",
    cap: 0,
    warning_threshold: 0.8,
    scope: "global",
    scope_id: null,
    action: "warn",
    enabled: true,
  };
}

export default function Limits({ onChange }: { onChange: () => void }) {
  const { t } = useI18n();
  const METRICS: { id: LimitMetric; label: string; unit: string; step: string }[] = [
    { id: "money", label: t("money"), unit: "$", step: "0.01" },
    { id: "tokens", label: t("tokens"), unit: t("tokens").toLowerCase(), step: "1" },
    { id: "requests", label: t("requests"), unit: t("requests").toLowerCase(), step: "1" },
    { id: "time_sec", label: t("time"), unit: t("hours"), step: "0.1" },
    { id: "requests_per_minute", label: t("requestsPerMinute"), unit: t("rpm"), step: "1" },
    { id: "tokens_per_minute", label: t("tokensPerMinute"), unit: t("tpm"), step: "1" },
  ];

  const PERIODS: { id: Exclude<LimitPeriod, { custom_sec: number }>; label: string }[] = [
    { id: "once", label: t("oneTime") },
    { id: "hourly", label: t("hourly") },
    { id: "daily", label: t("daily") },
    { id: "weekly", label: t("weekly") },
    { id: "monthly", label: t("monthly") },
  ];

  const SCOPES: { id: LimitScope; label: string }[] = [
    { id: "global", label: t("global") },
    { id: "provider", label: t("provider") },
    { id: "project", label: t("project") },
  ];

  const ACTIONS: { id: LimitAction; label: string }[] = [
    { id: "warn", label: t("warnOnly") },
    { id: "block", label: t("blockRequests") },
    { id: "pause", label: t("pauseProxy") },
  ];

  const [limits, setLimits] = useState<Limit[]>([]);
  const [status, setStatus] = useState<LimitStatus[]>([]);
  const [providers, setProviders] = useState<SimpleProvider[]>([]);
  const [projects, setProjects] = useState<SimpleProject[]>([]);
  const [presets, setPresets] = useState<LimitPreset[]>([]);
  const [form, setForm] = useState<LimitInput>(blankLimit());
  const [editingId, setEditingId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isCustomPeriod, setIsCustomPeriod] = useState(false);
  const [customPeriodSec, setCustomPeriodSec] = useState("");

  const refresh = useCallback(() => {
    invoke<Limit[]>("list_limits").then(setLimits).catch(console.error);
    invoke<LimitStatus[]>("get_limit_status").then(setStatus).catch(console.error);
    invoke<{ provider: SimpleProvider }[]>("list_providers")
      .then((dtos) => setProviders(dtos.map((d) => ({ id: d.provider.id, name: d.provider.name }))))
      .catch(console.error);
    invoke<SimpleProject[]>("list_projects").then(setProjects).catch(console.error);
    invoke<LimitPreset[]>("get_limit_presets").then(setPresets).catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
    const i = setInterval(refresh, 4000);
    return () => clearInterval(i);
  }, [refresh]);

  const statusFor = (id: number) => status.find((s) => s.limit.id === id);

  const startEdit = (l: Limit) => {
    setForm({ ...l });
    setEditingId(l.id);
    if (typeof l.period !== "string") {
      setIsCustomPeriod(true);
      setCustomPeriodSec(String(l.period.custom_sec));
    } else {
      setIsCustomPeriod(false);
      setCustomPeriodSec("");
    }
    setError(null);
  };

  const cancelEdit = () => {
    setForm(blankLimit());
    setEditingId(null);
    setIsCustomPeriod(false);
    setCustomPeriodSec("");
  };

  const applyPreset = (p: LimitPreset) => {
    setForm({ ...p.input });
    setIsCustomPeriod(false);
    setCustomPeriodSec("");
  };

  const buildInput = (): LimitInput | null => {
    const period: LimitPeriod = isCustomPeriod
      ? { custom_sec: Number(customPeriodSec) || 0 }
      : (form.period as Exclude<LimitPeriod, { custom_sec: number }>);
    if (isCustomPeriod && (Number(customPeriodSec) || 0) <= 0) {
      setError(t("customPeriodPositive"));
      return null;
    }
    return {
      ...form,
      period,
      cap: Number(form.cap) || 0,
      warning_threshold: Number(form.warning_threshold) || 0,
      scope_id: form.scope === "global" ? null : form.scope_id,
    };
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    const input = buildInput();
    if (!input) return;
    try {
      if (editingId !== null) {
        await invoke("update_limit", { id: editingId, input });
      } else {
        await invoke("add_limit", { input });
      }
      cancelEdit();
      refresh();
      onChange();
    } catch (err) {
      setError(String(err));
    }
  };

  const remove = async (id: number, name: string) => {
    if (!confirm(t("deleteLimitConfirm").replace("{name}", name))) return;
    if (editingId === id) cancelEdit();
    await invoke("delete_limit", { id });
    refresh();
    onChange();
  };

  const metricInfo = METRICS.find((m) => m.id === form.metric)!;

  return (
    <div className="grid grid-cols-1 gap-5 lg:grid-cols-2">
      <div className="space-y-4">
        <h2 className="text-sm font-semibold text-neutral-200">{t("configuredLimits")}</h2>
        <div className="space-y-2">
          {limits.length === 0 && (
            <p className="text-xs text-neutral-600">{t("noLimitsYet")}</p>
          )}
          {limits.map((l) => {
            const s = statusFor(l.id);
            const ratio = s ? Math.min(s.ratio, 1) : 0;
            const color =
              ratio >= 1 ? "bg-red-500" : ratio >= l.warning_threshold ? "bg-amber-500" : "bg-emerald-500";
            return (
              <div
                key={l.id}
                className={`rounded-lg border px-3 py-2 ${
                  editingId === l.id
                    ? "border-emerald-600 bg-neutral-900/60"
                    : "border-neutral-800 bg-neutral-900/40"
                }`}
              >
                <div className="flex items-center justify-between">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="text-sm font-medium text-neutral-200">{l.name}</span>
                      {!l.enabled && (
                        <span className="rounded bg-neutral-700 px-1.5 py-0.5 text-[10px] text-neutral-300">
                          {t("disabled")}
                        </span>
                      )}
                      <span
                        className={`rounded px-1.5 py-0.5 text-[10px] ${
                          l.action === "block"
                            ? "bg-red-500/20 text-red-300"
                            : l.action === "pause"
                            ? "bg-amber-500/20 text-amber-300"
                            : "bg-sky-500/20 text-sky-300"
                        }`}
                      >
                        {t(l.action)}
                      </span>
                    </div>
                    <div className="truncate font-mono text-[11px] text-neutral-500">
                      {formatMetricValue(l.metric, l.cap)} / {periodLabel(l.period, t)} · {SCOPES.find((s) => s.id === l.scope)?.label ?? l.scope}
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => startEdit(l)}
                      title="Edit"
                      className="text-neutral-500 hover:text-emerald-400"
                    >
                      ✎
                    </button>
                    <button
                      onClick={() => remove(l.id, l.name)}
                      className="text-neutral-500 hover:text-red-400"
                    >
                      ✕
                    </button>
                  </div>
                </div>
                {s && l.enabled && (
                  <div className="mt-2">
                    <div className="flex justify-between text-[10px] text-neutral-400">
                      <span>
                        {formatMetricValue(l.metric, s.used)} / {formatMetricValue(l.metric, l.cap)}
                      </span>
                      <span>{(ratio * 100).toFixed(0)}%</span>
                    </div>
                    <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-neutral-800">
                      <div
                        className={`h-full transition-all ${color}`}
                        style={{ width: `${ratio * 100}%` }}
                      />
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>

      <div className="space-y-4">
        <form
          onSubmit={submit}
          className="space-y-3 rounded-lg border border-neutral-800 bg-neutral-900/40 p-4"
        >
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-semibold text-neutral-200">
              {editingId !== null ? t("editLimit") : t("addLimit")}
            </h2>
            {editingId !== null && (
              <button
                type="button"
                onClick={cancelEdit}
                className="text-[11px] text-neutral-500 hover:text-neutral-300"
              >
                {t("cancel")}
              </button>
            )}
          </div>

          {!editingId && (
            <div className="flex flex-wrap gap-1">
              {presets.map((p) => (
                <button
                  key={p.name}
                  type="button"
                  onClick={() => applyPreset(p)}
                  className="rounded bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 hover:bg-neutral-700"
                >
                  {p.name}
                </button>
              ))}
            </div>
          )}

          <Field label={t("name")}>
            <input
              required
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              className={inputCls}
              placeholder={t("limitNamePlaceholder")}
            />
          </Field>

          <div className="grid grid-cols-2 gap-2">
            <Field label={t("metric")}>
              <select
                value={form.metric}
                onChange={(e) => setForm({ ...form, metric: e.target.value as LimitMetric })}
                className={inputCls}
              >
                {METRICS.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label}
                  </option>
                ))}
              </select>
            </Field>
            <Field label={`${t("cap")} (${metricInfo.unit})`}>
              <input
                type="number"
                step={metricInfo.step}
                min={0}
                value={displayCap(form.metric, form.cap)}
                onChange={(e) =>
                  setForm({
                    ...form,
                    cap: storeCap(form.metric, Number(e.target.value)),
                  })
                }
                className={inputCls}
              />
            </Field>
          </div>

          <div className="grid grid-cols-2 gap-2">
            <Field label={t("period")}>
              {!isCustomPeriod ? (
                <select
                  value={form.period as string}
                  onChange={(e) => {
                    if (e.target.value === "custom") {
                      setIsCustomPeriod(true);
                    } else {
                      setForm({ ...form, period: e.target.value as any });
                    }
                  }}
                  className={inputCls}
                >
                  {PERIODS.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                    </option>
                  ))}
                  <option value="custom">{t("customSeconds")}</option>
                </select>
              ) : (
                <div className="flex gap-2">
                  <input
                    type="number"
                    min={1}
                    value={customPeriodSec}
                    onChange={(e) => setCustomPeriodSec(e.target.value)}
                    placeholder={t("seconds")}
                    className={inputCls}
                  />
                  <button
                    type="button"
                    onClick={() => setIsCustomPeriod(false)}
                    className="text-[11px] text-neutral-500 hover:text-neutral-300"
                  >
                    {t("presets")}
                  </button>
                </div>
              )}
            </Field>
            <Field label={t("warningAt")}>
              <input
                type="number"
                step="5"
                min={0}
                max={100}
                value={Math.round((form.warning_threshold || 0) * 100)}
                onChange={(e) =>
                  setForm({
                    ...form,
                    warning_threshold: Number(e.target.value) / 100,
                  })
                }
                className={inputCls}
              />
            </Field>
          </div>

          <div className="grid grid-cols-2 gap-2">
            <Field label={t("scope")}>
              <select
                value={form.scope}
                onChange={(e) =>
                  setForm({
                    ...form,
                    scope: e.target.value as LimitScope,
                    scope_id: e.target.value === "global" ? null : form.scope_id,
                  })
                }
                className={inputCls}
              >
                {SCOPES.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.label}
                  </option>
                ))}
              </select>
            </Field>
            {form.scope !== "global" && (
              <Field label={form.scope === "provider" ? t("provider") : t("project")}>
                <select
                  value={form.scope_id ?? ""}
                  onChange={(e) =>
                    setForm({ ...form, scope_id: Number(e.target.value) || null })
                  }
                  className={inputCls}
                  required
                >
                  <option value="">{t("select")}</option>
                  {(form.scope === "provider" ? providers : projects).map((x) => (
                    <option key={x.id} value={x.id}>
                      {x.name}
                    </option>
                  ))}
                </select>
              </Field>
            )}
          </div>

          <div className="grid grid-cols-2 gap-2">
            <Field label={t("actionWhenExceeded")}>
              <select
                value={form.action}
                onChange={(e) => setForm({ ...form, action: e.target.value as LimitAction })}
                className={inputCls}
              >
                {ACTIONS.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.label}
                  </option>
                ))}
              </select>
            </Field>
            <label className="flex items-center gap-2 self-end text-xs text-neutral-400">
              <input
                type="checkbox"
                checked={form.enabled}
                onChange={(e) => setForm({ ...form, enabled: e.target.checked })}
              />
              {t("enabled")}
            </label>
          </div>

          {error && <p className="text-xs text-red-400">{error}</p>}
          <button
            type="submit"
            className="w-full rounded-md bg-emerald-500/20 py-2 text-xs font-semibold text-emerald-300 hover:bg-emerald-500/30"
          >
            {editingId !== null ? t("saveChanges") : t("addLimit")}
          </button>
        </form>

        <section className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4 text-[11px] leading-relaxed text-neutral-500">
          <h2 className="mb-2 text-sm font-semibold text-neutral-300">{t("howLimitsWork")}</h2>
          <ul className="list-inside list-disc space-y-1">
            <li>{t("limitsHelp1")}</li>
            <li>{t("limitsHelp2")}</li>
            <li>{t("limitsHelp3")}</li>
            <li>{t("limitsHelp4")}</li>
          </ul>
        </section>
      </div>
    </div>
  );
}

const inputCls =
  "w-full rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 focus:border-emerald-500 focus:outline-none dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200";

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1 block text-[11px] text-neutral-500">{label}</span>
      {children}
    </label>
  );
}
