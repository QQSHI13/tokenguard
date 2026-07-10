import { useEffect, useState, useCallback } from "react";
import type { FormEvent, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

type ProviderFormat = "openai" | "anthropic" | "google";
type AuthScheme = "bearer" | "x_api_key" | "api_key" | "x_goog_api_key";

type ModelMapping = {
  local: string;
  remote: string;
  input_cost_per_1k: number | null;
  output_cost_per_1k: number | null;
  cached_input_cost_per_1k: number | null;
};

type Provider = {
  id: number;
  name: string;
  base_url: string;
  format: ProviderFormat;
  auth: AuthScheme;
  models: ModelMapping[];
  is_default: boolean;
  fallback_provider_id: number | null;
  extra_headers: [string, string][];
};
type ProviderDto = {
  provider: Provider;
  api_key_set: boolean;
  key_error: string | null;
  key_created_at: string | null;
};

type ProviderHealth = {
  ok: boolean;
  latency_ms: number;
  error: string | null;
  checked_at: string;
};

type Input = {
  name: string;
  base_url: string;
  format: ProviderFormat;
  auth: AuthScheme;
  api_key: string;
  models: ModelMapping[];
  is_default: boolean;
  clear_key: boolean;
  fallback_provider_id: number | null;
  extra_headers: [string, string][];
};

const PRESETS: {
  name: string;
  base_url: string;
  format: ProviderFormat;
  auth: AuthScheme;
  models: ModelMapping[];
}[] = [
  {
    name: "OpenAI",
    base_url: "https://api.openai.com",
    format: "openai",
    auth: "bearer",
    models: [
      { local: "gpt-4o", remote: "gpt-4o", input_cost_per_1k: null, output_cost_per_1k: null, cached_input_cost_per_1k: null },
      { local: "gpt-4o-mini", remote: "gpt-4o-mini", input_cost_per_1k: null, output_cost_per_1k: null, cached_input_cost_per_1k: null },
      { local: "gpt-4-turbo", remote: "gpt-4-turbo", input_cost_per_1k: null, output_cost_per_1k: null, cached_input_cost_per_1k: null },
      { local: "gpt-3.5-turbo", remote: "gpt-3.5-turbo", input_cost_per_1k: null, output_cost_per_1k: null, cached_input_cost_per_1k: null },
    ],
  },
  {
    name: "Anthropic",
    base_url: "https://api.anthropic.com",
    format: "anthropic",
    auth: "x_api_key",
    models: [
      { local: "claude-sonnet-4", remote: "claude-sonnet-4-20250514", input_cost_per_1k: null, output_cost_per_1k: null, cached_input_cost_per_1k: null },
      { local: "claude-3-5-sonnet", remote: "claude-3-5-sonnet-20241022", input_cost_per_1k: null, output_cost_per_1k: null, cached_input_cost_per_1k: null },
      { local: "claude-3-5-haiku", remote: "claude-3-5-haiku-20241022", input_cost_per_1k: null, output_cost_per_1k: null, cached_input_cost_per_1k: null },
    ],
  },
  {
    name: "Google",
    base_url: "https://generativelanguage.googleapis.com",
    format: "google",
    auth: "x_goog_api_key",
    models: [
      { local: "gemini-1.5-pro", remote: "gemini-1.5-pro", input_cost_per_1k: null, output_cost_per_1k: null, cached_input_cost_per_1k: null },
      { local: "gemini-1.5-flash", remote: "gemini-1.5-flash", input_cost_per_1k: null, output_cost_per_1k: null, cached_input_cost_per_1k: null },
      { local: "gemini-2.0-flash", remote: "gemini-2.0-flash", input_cost_per_1k: null, output_cost_per_1k: null, cached_input_cost_per_1k: null },
    ],
  },
  {
    name: "OpenRouter",
    base_url: "https://openrouter.ai/api",
    format: "openai",
    auth: "bearer",
    models: [],
  },
];

function isKeyOld(createdAt: string | null, thresholdDays: number): boolean {
  if (!createdAt) return false;
  const created = new Date(createdAt);
  if (isNaN(created.getTime())) return false;
  const days = (Date.now() - created.getTime()) / (1000 * 60 * 60 * 24);
  return days >= thresholdDays;
}

function blank(): Input {
  return {
    name: "",
    base_url: "https://api.openai.com",
    format: "openai",
    auth: "bearer",
    api_key: "",
    models: [],
    is_default: true,
    clear_key: false,
    fallback_provider_id: null,
    extra_headers: [],
  };
}

function fromProvider(p: Provider): Input {
  return {
    name: p.name,
    base_url: p.base_url,
    format: p.format,
    auth: p.auth,
    api_key: "", // never have the key client-side; blank = keep current
    models: [...p.models],
    is_default: p.is_default,
    clear_key: false,
    fallback_provider_id: p.fallback_provider_id ?? null,
    extra_headers: [...p.extra_headers],
  };
}

export default function Providers({ onChange }: { onChange: () => void }) {
  const { t } = useI18n();
  const [providers, setProviders] = useState<ProviderDto[]>([]);
  const [form, setForm] = useState<Input>(blank());
  const [editingId, setEditingId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState<number | null>(null);
  const [health, setHealth] = useState<Record<number, ProviderHealth>>({});
  const [keyRotationDays, setKeyRotationDays] = useState(90);

  const refresh = useCallback(() => {
    invoke<ProviderDto[]>("list_providers").then(setProviders).catch(console.error);
    invoke<Record<number, ProviderHealth>>("get_provider_healths")
      .then(setHealth)
      .catch(console.error);
    invoke<{ key_rotation_days: number }>("get_settings")
      .then((s) => setKeyRotationDays(s.key_rotation_days || 90))
      .catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const startEdit = (p: Provider) => {
    setForm(fromProvider(p));
    setEditingId(p.id);
    setError(null);
  };

  const cancelEdit = () => {
    setForm(blank());
    setEditingId(null);
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      if (editingId !== null) {
        await invoke("update_provider", { id: editingId, input: form });
      } else {
        await invoke("add_provider", { input: form });
      }
      cancelEdit();
      refresh();
      onChange();
    } catch (err) {
      setError(String(err));
    }
  };

  const remove = async (id: number, name: string) => {
    if (!confirm(t("deleteProviderConfirm").replace("{name}", name)))
      return;
    if (editingId === id) cancelEdit();
    await invoke("delete_provider", { id });
    refresh();
    onChange();
  };

  const refreshModels = async (id: number) => {
    setRefreshing(id);
    try {
      await invoke<string[]>("refresh_models", { id });
      refresh();
      onChange();
    } catch (err) {
      alert(t("modelRefreshFailed") + String(err));
    } finally {
      setRefreshing(null);
    }
  };

  const applyPreset = (p: (typeof PRESETS)[number]) => {
    setForm((f) => ({
      ...f,
      name: p.name,
      base_url: p.base_url,
      format: p.format,
      auth: p.auth,
      models: [...p.models],
    }));
  };

  const editing = editingId !== null;

  return (
    <div className="grid grid-cols-1 gap-5 lg:grid-cols-2">
      <div>
        <h2 className="mb-3 text-sm font-semibold text-neutral-200">
          {t("configuredProviders")}
        </h2>
        <div className="space-y-2">
          {providers.length === 0 && (
            <p className="text-xs text-neutral-600">{t("noProvidersYet")}</p>
          )}
          {providers.map(({ provider: p, api_key_set, key_error, key_created_at }) => (
            <div
              key={p.id}
              className={`flex items-center justify-between rounded-lg border px-3 py-2 ${
                editingId === p.id
                  ? "border-emerald-600 bg-neutral-900/60"
                  : "border-neutral-800 bg-neutral-900/40"
              }`}
            >
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-sm font-medium text-neutral-200">
                    {p.name}
                  </span>
                  <HealthDot health={health[p.id]} t={t} />
                  {p.is_default && (
                    <span className="rounded bg-emerald-500/20 px-1.5 py-0.5 text-[10px] text-emerald-300">
                      {t("default")}
                    </span>
                  )}
                  {key_error ? (
                    <span
                      title={key_error}
                      className="rounded bg-red-500/20 px-1.5 py-0.5 text-[10px] text-red-300"
                    >
                      {t("keyError")}
                    </span>
                  ) : api_key_set ? (
                    <span className="rounded bg-sky-500/20 px-1.5 py-0.5 text-[10px] text-sky-300">
                      {t("keySet")}
                    </span>
                  ) : (
                    <span className="rounded bg-red-500/20 px-1.5 py-0.5 text-[10px] text-red-300">
                      {t("noKey")}
                    </span>
                  )}
                  {api_key_set && isKeyOld(key_created_at, keyRotationDays) && (
                    <span
                      title={t("keyRotationWarning")}
                      className="rounded bg-amber-500/20 px-1.5 py-0.5 text-[10px] text-amber-300"
                    >
                      {t("rotateKey")}
                    </span>
                  )}
                </div>
                <div className="truncate font-mono text-[11px] text-neutral-500">
                  {p.base_url} · {p.format} · {p.auth}
                </div>
                {p.models.length > 0 && (
                  <div className="mt-0.5 truncate font-mono text-[11px] text-neutral-600">
                    {p.models.map((m) => m.local).join(", ")}
                  </div>
                )}
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => startEdit(p)}
                  title="Edit"
                  className="text-neutral-500 hover:text-emerald-400"
                >
                  ✎
                </button>
                <button
                  onClick={() => refreshModels(p.id)}
                  disabled={refreshing === p.id}
                  title="Fetch /v1/models"
                  className="text-neutral-500 hover:text-sky-400 disabled:opacity-40"
                >
                  {refreshing === p.id ? "⋯" : "↻"}
                </button>
                <button
                  onClick={() => remove(p.id, p.name)}
                  className="text-neutral-500 hover:text-red-400"
                >
                  ✕
                </button>
              </div>
            </div>
          ))}
        </div>
      </div>

      <form
        onSubmit={submit}
        className="space-y-3 rounded-lg border border-neutral-800 bg-neutral-900/40 p-4"
      >
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-semibold text-neutral-200">
            {editing ? t("editProvider") : t("addProvider")}
          </h2>
          {editing && (
            <button
              type="button"
              onClick={cancelEdit}
              className="text-[11px] text-neutral-500 hover:text-neutral-300"
            >
              {t("cancel")}
            </button>
          )}
        </div>

        {!editing && (
          <div className="flex flex-wrap gap-1">
            {PRESETS.map((p) => (
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
          />
        </Field>
        <Field label={t("baseUrl")}>
          <input
            required
            value={form.base_url}
            onChange={(e) => setForm({ ...form, base_url: e.target.value })}
            className={inputCls}
          />
        </Field>
        <div className="grid grid-cols-2 gap-2">
          <Field label={t("format")}>
            <select
              value={form.format}
              onChange={(e) => {
                const format = e.target.value as ProviderFormat;
                const defaultAuth: AuthScheme =
                  format === "anthropic"
                    ? "x_api_key"
                    : format === "google"
                    ? "x_goog_api_key"
                    : "bearer";
                setForm({
                  ...form,
                  format,
                  auth: defaultAuth,
                });
              }}
              className={inputCls}
            >
              <option value="openai">openai</option>
              <option value="anthropic">anthropic</option>
              <option value="google">google</option>
            </select>
          </Field>
          <Field label={t("authHeader")}>
            <select
              value={form.auth}
              onChange={(e) => setForm({ ...form, auth: e.target.value as AuthScheme })}
              className={inputCls}
            >
              <option value="bearer">Authorization: Bearer</option>
              <option value="x_api_key">x-api-key (Anthropic)</option>
              <option value="api_key">api-key (Azure)</option>
              <option value="x_goog_api_key">x-goog-api-key (Google)</option>
            </select>
          </Field>
        </div>
        <Field
          label={
            editing
              ? t("apiKeyKeepCurrent")
              : t("apiKeyStored")
          }
        >
          <input
            type="password"
            value={form.api_key}
            onChange={(e) => setForm({ ...form, api_key: e.target.value, clear_key: false })}
            className={inputCls}
            placeholder={editing ? "•••••• unchanged" : "sk-..."}
            disabled={form.clear_key}
          />
        </Field>
        {editing && (
          <label className="flex items-center gap-2 text-xs text-neutral-400">
            <input
              type="checkbox"
              checked={form.clear_key}
              onChange={(e) => setForm({ ...form, clear_key: e.target.checked })}
            />
            {t("clearStoredKey")}
          </label>
        )}
        <div>
          <div className="mb-1 flex items-center justify-between">
            <label className="text-[11px] text-neutral-500">{t("models")}</label>
          </div>
          <div className="space-y-2">
            {form.models.map((m, i) => (
              <div key={i} className="grid grid-cols-[1fr_1fr_1fr_1fr_1fr_auto] gap-2">
                <input
                  value={m.local}
                  onChange={(e) => {
                    const next = [...form.models];
                    next[i] = { ...next[i], local: e.target.value };
                    setForm({ ...form, models: next });
                  }}
                  placeholder={t("localName")}
                  className={inputCls}
                />
                <input
                  value={m.remote}
                  onChange={(e) => {
                    const next = [...form.models];
                    next[i] = { ...next[i], remote: e.target.value };
                    setForm({ ...form, models: next });
                  }}
                  placeholder={t("providerName")}
                  className={inputCls}
                />
                <input
                  type="number"
                  step="0.0001"
                  value={m.input_cost_per_1k ?? ""}
                  onChange={(e) => {
                    const next = [...form.models];
                    next[i] = {
                      ...next[i],
                      input_cost_per_1k: e.target.value ? Number(e.target.value) : null,
                    };
                    setForm({ ...form, models: next });
                  }}
                  placeholder={t("inputCost")}
                  className={inputCls}
                />
                <input
                  type="number"
                  step="0.0001"
                  value={m.output_cost_per_1k ?? ""}
                  onChange={(e) => {
                    const next = [...form.models];
                    next[i] = {
                      ...next[i],
                      output_cost_per_1k: e.target.value ? Number(e.target.value) : null,
                    };
                    setForm({ ...form, models: next });
                  }}
                  placeholder={t("outputCost")}
                  className={inputCls}
                />
                <input
                  type="number"
                  step="0.0001"
                  value={m.cached_input_cost_per_1k ?? ""}
                  onChange={(e) => {
                    const next = [...form.models];
                    next[i] = {
                      ...next[i],
                      cached_input_cost_per_1k: e.target.value
                        ? Number(e.target.value)
                        : null,
                    };
                    setForm({ ...form, models: next });
                  }}
                  placeholder={t("cachedInputCost")}
                  className={inputCls}
                />
                <button
                  type="button"
                  onClick={() => {
                    const next = form.models.filter((_, idx) => idx !== i);
                    setForm({ ...form, models: next });
                  }}
                  className="px-2 text-neutral-500 hover:text-red-400"
                >
                  ✕
                </button>
              </div>
            ))}
            <button
              type="button"
              onClick={() =>
                setForm({
                  ...form,
                  models: [
                    ...form.models,
                    {
                      local: "",
                      remote: "",
                      input_cost_per_1k: null,
                      output_cost_per_1k: null,
                      cached_input_cost_per_1k: null,
                    },
                  ],
                })
              }
              className="w-full rounded-md border border-dashed border-neutral-400 py-1.5 text-[11px] text-neutral-500 hover:border-neutral-600 hover:text-neutral-700 dark:border-neutral-600 dark:hover:border-neutral-400 dark:hover:text-neutral-300"
            >
              {t("addModel")}
            </button>
          </div>
          <p className="mt-1 text-[10px] text-neutral-500">
            {t("modelAliasHint")}
          </p>
        </div>
        <label className="flex items-center gap-2 text-xs text-neutral-400">
          <input
            type="checkbox"
            checked={form.is_default}
            onChange={(e) => setForm({ ...form, is_default: e.target.checked })}
          />
          {t("defaultForFormat")}
        </label>
        <Field label={t("fallbackProvider")}>
          <select
            value={form.fallback_provider_id ?? ""}
            onChange={(e) =>
              setForm({
                ...form,
                fallback_provider_id: Number(e.target.value) || null,
              })
            }
            className={inputCls}
          >
            <option value="">{t("noFallback")}</option>
            {providers
              .filter(({ provider: p }) => p.id !== editingId)
              .map(({ provider: p }) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
          </select>
        </Field>

        <div>
          <div className="mb-1 flex items-center justify-between">
            <label className="text-[11px] text-neutral-500">{t("extraHeaders")}</label>
          </div>
          <div className="space-y-2">
            {form.extra_headers.map((h, i) => (
              <div key={i} className="grid grid-cols-[1fr_1fr_auto] gap-2">
                <input
                  value={h[0]}
                  onChange={(e) => {
                    const next: [string, string][] = [...form.extra_headers];
                    next[i] = [e.target.value, h[1]];
                    setForm({ ...form, extra_headers: next });
                  }}
                  placeholder={t("headerName")}
                  className={inputCls}
                />
                <input
                  value={h[1]}
                  onChange={(e) => {
                    const next: [string, string][] = [...form.extra_headers];
                    next[i] = [h[0], e.target.value];
                    setForm({ ...form, extra_headers: next });
                  }}
                  placeholder={t("headerValue")}
                  className={inputCls}
                />
                <button
                  type="button"
                  onClick={() => {
                    const next = form.extra_headers.filter((_, idx) => idx !== i);
                    setForm({ ...form, extra_headers: next });
                  }}
                  className="px-2 text-neutral-500 hover:text-red-400"
                >
                  ✕
                </button>
              </div>
            ))}
            <button
              type="button"
              onClick={() =>
                setForm({
                  ...form,
                  extra_headers: [...form.extra_headers, ["", ""]],
                })
              }
              className="w-full rounded-md border border-dashed border-neutral-400 py-1.5 text-[11px] text-neutral-500 hover:border-neutral-600 hover:text-neutral-700 dark:border-neutral-600 dark:hover:border-neutral-400 dark:hover:text-neutral-300"
            >
              {t("addHeader")}
            </button>
          </div>
          <p className="mt-1 text-[10px] text-neutral-500">{t("extraHeadersHint")}</p>
        </div>

        {error && <p className="text-xs text-red-400">{error}</p>}
        <button
          type="submit"
          className="w-full rounded-md bg-emerald-500/20 py-2 text-xs font-semibold text-emerald-300 hover:bg-emerald-500/30"
        >
          {editing ? t("saveChanges") : t("addProvider")}
        </button>
      </form>
    </div>
  );
}

const inputCls =
  "w-full rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 focus:border-emerald-500 focus:outline-none dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200";

function HealthDot({
  health,
  t,
}: {
  health: ProviderHealth | undefined;
  t: (
    key: keyof typeof import("../i18n").translations.en,
    vars?: Record<string, string | number>,
  ) => string;
}) {
  if (!health) {
    return (
      <span
        title={t("healthUnknown")}
        className="h-2 w-2 rounded-full bg-neutral-600"
      />
    );
  }
  if (health.ok) {
    return (
      <span
        title={t("healthy", { ms: health.latency_ms })}
        className="h-2 w-2 rounded-full bg-emerald-500"
      />
    );
  }
  return (
    <span
      title={health.error ?? t("unhealthy")}
      className="h-2 w-2 rounded-full bg-red-500"
    />
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1 block text-[11px] text-neutral-500">{label}</span>
      {children}
    </label>
  );
}
