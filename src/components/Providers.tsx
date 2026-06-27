import { useEffect, useState, useCallback } from "react";
import type { FormEvent, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

type ProviderFormat = "openai" | "anthropic";
type AuthScheme = "bearer" | "x_api_key" | "api_key";

type Provider = {
  id: number;
  name: string;
  base_url: string;
  format: ProviderFormat;
  auth: AuthScheme;
  models: string[];
  input_cost_per_1k: number | null;
  output_cost_per_1k: number | null;
  is_default: boolean;
};
type ProviderDto = { provider: Provider; api_key_set: boolean };

type Input = {
  name: string;
  base_url: string;
  format: ProviderFormat;
  auth: AuthScheme;
  api_key: string;
  models: string[];
  input_cost_per_1k: number | null;
  output_cost_per_1k: number | null;
  is_default: boolean;
};

const PRESETS: {
  name: string;
  base_url: string;
  format: ProviderFormat;
  auth: AuthScheme;
  models: string;
}[] = [
  {
    name: "OpenAI",
    base_url: "https://api.openai.com",
    format: "openai",
    auth: "bearer",
    models: "gpt-4o,gpt-4o-mini,gpt-4-turbo,gpt-3.5-turbo",
  },
  {
    name: "Anthropic",
    base_url: "https://api.anthropic.com",
    format: "anthropic",
    auth: "x_api_key",
    models: "claude-3-5-sonnet-20241022,claude-3-5-haiku-20241022,claude-3-opus-20240229",
  },
  {
    name: "OpenRouter",
    base_url: "https://openrouter.ai/api",
    format: "openai",
    auth: "bearer",
    models: "",
  },
];

export default function Providers({ onChange }: { onChange: () => void }) {
  const [providers, setProviders] = useState<ProviderDto[]>([]);
  const [form, setForm] = useState<Input>(blank());
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    invoke<ProviderDto[]>("list_providers").then(setProviders).catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await invoke("add_provider", { input: form });
      setForm(blank());
      refresh();
      onChange();
    } catch (err) {
      setError(String(err));
    }
  };

  const remove = async (id: number, name: string) => {
    if (!confirm(`Delete provider "${name}"? Its keychain entry will be removed.`))
      return;
    await invoke("delete_provider", { id });
    refresh();
    onChange();
  };

  const [refreshing, setRefreshing] = useState<number | null>(null);
  const refreshModels = async (id: number) => {
    setRefreshing(id);
    try {
      await invoke<string[]>("refresh_models", { id });
      refresh();
      onChange();
    } catch (err) {
      alert("Model refresh failed: " + String(err));
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
      models: p.models ? p.models.split(",") : [],
    }));
  };

  return (
    <div className="grid grid-cols-1 gap-5 lg:grid-cols-2">
      <div>
        <h2 className="mb-3 text-sm font-semibold text-neutral-200">
          Configured providers
        </h2>
        <div className="space-y-2">
          {providers.length === 0 && (
            <p className="text-xs text-neutral-600">
              No providers yet. Add one →
            </p>
          )}
          {providers.map(({ provider: p, api_key_set }) => (
            <div
              key={p.id}
              className="flex items-center justify-between rounded-lg border border-neutral-800 bg-neutral-900/40 px-3 py-2"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium text-neutral-200">
                    {p.name}
                  </span>
                  {p.is_default && (
                    <span className="rounded bg-emerald-500/20 px-1.5 py-0.5 text-[10px] text-emerald-300">
                      default
                    </span>
                  )}
                  <span
                    className={`rounded px-1.5 py-0.5 text-[10px] ${
                      api_key_set
                        ? "bg-sky-500/20 text-sky-300"
                        : "bg-red-500/20 text-red-300"
                    }`}
                  >
                    {api_key_set ? "key set" : "no key"}
                  </span>
                </div>
                <div className="truncate font-mono text-[11px] text-neutral-500">
                  {p.base_url} · {p.format} · {p.auth}
                </div>
                {p.models.length > 0 && (
                  <div className="mt-0.5 truncate font-mono text-[11px] text-neutral-600">
                    {p.models.join(", ")}
                  </div>
                )}
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => refreshModels(p.id)}
                  disabled={refreshing === p.id}
                  title="Fetch /v1/models from this provider"
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
        <h2 className="text-sm font-semibold text-neutral-200">Add provider</h2>

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

        <Field label="Name">
          <input
            required
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            className={inputCls}
          />
        </Field>
        <Field label="Base URL">
          <input
            required
            value={form.base_url}
            onChange={(e) => setForm({ ...form, base_url: e.target.value })}
            className={inputCls}
          />
        </Field>
        <div className="grid grid-cols-2 gap-2">
          <Field label="Format">
            <select
              value={form.format}
              onChange={(e) => {
                const format = e.target.value as ProviderFormat;
                setForm({
                  ...form,
                  format,
                  auth: format === "anthropic" ? "x_api_key" : "bearer",
                });
              }}
              className={inputCls}
            >
              <option value="openai">openai</option>
              <option value="anthropic">anthropic</option>
            </select>
          </Field>
          <Field label="Auth header">
            <select
              value={form.auth}
              onChange={(e) =>
                setForm({ ...form, auth: e.target.value as AuthScheme })
              }
              className={inputCls}
            >
              <option value="bearer">Authorization: Bearer</option>
              <option value="x_api_key">x-api-key (Anthropic)</option>
              <option value="api_key">api-key (Azure)</option>
            </select>
          </Field>
        </div>
        <Field label="API key (stored in OS keychain)">
          <input
            type="password"
            value={form.api_key}
            onChange={(e) => setForm({ ...form, api_key: e.target.value })}
            className={inputCls}
            placeholder="sk-..."
          />
        </Field>
        <Field label="Models (comma-separated, used for routing)">
          <input
            value={form.models.join(",")}
            onChange={(e) =>
              setForm({
                ...form,
                models: e.target.value
                  .split(",")
                  .map((s) => s.trim())
                  .filter(Boolean),
              })
            }
            className={inputCls}
            placeholder="gpt-4o, gpt-4o-mini"
          />
        </Field>
        <div className="grid grid-cols-2 gap-2">
          <Field label="Input $/1K (optional)">
            <input
              type="number"
              step="0.0001"
              value={form.input_cost_per_1k ?? ""}
              onChange={(e) =>
                setForm({
                  ...form,
                  input_cost_per_1k: e.target.value
                    ? Number(e.target.value)
                    : null,
                })
              }
              className={inputCls}
            />
          </Field>
          <Field label="Output $/1K (optional)">
            <input
              type="number"
              step="0.0001"
              value={form.output_cost_per_1k ?? ""}
              onChange={(e) =>
                setForm({
                  ...form,
                  output_cost_per_1k: e.target.value
                    ? Number(e.target.value)
                    : null,
                })
              }
              className={inputCls}
            />
          </Field>
        </div>
        <label className="flex items-center gap-2 text-xs text-neutral-400">
          <input
            type="checkbox"
            checked={form.is_default}
            onChange={(e) => setForm({ ...form, is_default: e.target.checked })}
          />
          Default for this format family (fallback when no model match)
        </label>

        {error && (
          <p className="text-xs text-red-400">{error}</p>
        )}
        <button
          type="submit"
          className="w-full rounded-md bg-emerald-500/20 py-2 text-xs font-semibold text-emerald-300 hover:bg-emerald-500/30"
        >
          Add provider
        </button>
      </form>
    </div>
  );
}

const inputCls =
  "w-full rounded-md border border-neutral-700 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-200 focus:border-emerald-500 focus:outline-none";

function Field({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <label className="block">
      <span className="mb-1 block text-[11px] text-neutral-500">{label}</span>
      {children}
    </label>
  );
}

function blank(): Input {
  return {
    name: "",
    base_url: "https://api.openai.com",
    format: "openai",
    auth: "bearer",
    api_key: "",
    models: [],
    input_cost_per_1k: null,
    output_cost_per_1k: null,
    is_default: true,
  };
}
