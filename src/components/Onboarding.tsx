import { useEffect, useState, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

type ProviderFormat = "openai" | "anthropic";
type AuthScheme = "bearer" | "x_api_key" | "api_key";

type ProviderInput = {
  name: string;
  base_url: string;
  format: ProviderFormat;
  auth: AuthScheme;
  api_key: string;
  models: {
    local: string;
    remote: string;
    input_cost_per_1k: number | null;
    output_cost_per_1k: number | null;
    cached_input_cost_per_1k: number | null;
  }[];
  is_default: boolean;
};

function randomLabelKey() {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let s = "tg_";
  for (let i = 0; i < 24; i++) {
    s += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return s;
}

export default function Onboarding({
  onComplete,
}: {
  onComplete: () => void;
}) {
  const { t } = useI18n();
  const [step, setStep] = useState<"welcome" | "provider" | "project" | "connect">("welcome");
  const [settings, setSettings] = useState<{ proxy_url: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copiedProxy, setCopiedProxy] = useState(false);
  const [copiedKey, setCopiedKey] = useState(false);

  const [provider, setProvider] = useState<ProviderInput>({
    name: "OpenAI",
    base_url: "https://api.openai.com",
    format: "openai",
    auth: "bearer",
    api_key: "",
    models: [
      {
        local: "gpt-4o-mini",
        remote: "gpt-4o-mini",
        input_cost_per_1k: null,
        output_cost_per_1k: null,
        cached_input_cost_per_1k: null,
      },
    ],
    is_default: true,
  });

  const [project, setProject] = useState({ name: "default", label_key: randomLabelKey() });

  useEffect(() => {
    invoke<{ proxy_url: string }>("get_settings")
      .then(setSettings)
      .catch(console.error);
  }, []);

  const submitProvider = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await invoke("add_provider", {
        input: { ...provider, clear_key: false },
      });
      setStep("project");
    } catch (err) {
      setError(String(err));
    }
  };

  const submitProject = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await invoke("add_project", {
        input: { name: project.name, label_key: project.label_key },
      });
      setStep("connect");
    } catch (err) {
      setError(String(err));
    }
  };

  const copy = async (text: string, setCopied: (v: boolean) => void) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // ignore
    }
  };

  const inputCls =
    "w-full rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 focus:border-emerald-500 focus:outline-none dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200";

  const labelCls = "mb-1 block text-[11px] text-neutral-500";

  return (
    <div className="flex h-full items-center justify-center p-4">
      <div className="w-full max-w-lg space-y-5 rounded-xl border border-neutral-200 bg-white p-6 shadow-sm dark:border-neutral-800 dark:bg-neutral-900/40">
        {step === "welcome" && (
          <>
            <div className="text-center">
              <div className="mb-3 text-4xl">🛡️</div>
              <h2 className="text-lg font-semibold">{t("welcomeTitle")}</h2>
              <p className="mt-2 text-sm text-neutral-600 dark:text-neutral-400">
                {t("welcomeSubtitle")}
              </p>
            </div>
            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={onComplete}
                className="rounded-md px-3 py-1.5 text-xs font-medium text-neutral-500 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
              >
                {t("skipOnboarding")}
              </button>
              <button
                type="button"
                onClick={() => setStep("provider")}
                className="rounded-md bg-emerald-500/20 px-4 py-1.5 text-xs font-semibold text-emerald-700 hover:bg-emerald-500/30 dark:text-emerald-300"
              >
                {t("getStarted")}
              </button>
            </div>
          </>
        )}

        {step === "provider" && (
          <form onSubmit={submitProvider} className="space-y-3">
            <div>
              <h2 className="text-base font-semibold">{t("onboardingProviderTitle")}</h2>
              <p className="text-xs text-neutral-500">{t("onboardingProviderSubtitle")}</p>
            </div>
            <label className={labelCls}>{t("name")}</label>
            <input
              required
              value={provider.name}
              onChange={(e) => setProvider({ ...provider, name: e.target.value })}
              className={inputCls}
            />
            <label className={labelCls}>{t("baseUrl")}</label>
            <input
              required
              value={provider.base_url}
              onChange={(e) => setProvider({ ...provider, base_url: e.target.value })}
              className={inputCls}
            />
            <div className="grid grid-cols-2 gap-2">
              <div>
                <label className={labelCls}>{t("format")}</label>
                <select
                  value={provider.format}
                  onChange={(e) => {
                    const format = e.target.value as ProviderFormat;
                    setProvider({
                      ...provider,
                      format,
                      auth: format === "anthropic" ? "x_api_key" : "bearer",
                    });
                  }}
                  className={inputCls}
                >
                  <option value="openai">openai</option>
                  <option value="anthropic">anthropic</option>
                </select>
              </div>
              <div>
                <label className={labelCls}>{t("authHeader")}</label>
                <select
                  value={provider.auth}
                  onChange={(e) =>
                    setProvider({ ...provider, auth: e.target.value as AuthScheme })
                  }
                  className={inputCls}
                >
                  <option value="bearer">Authorization: Bearer</option>
                  <option value="x_api_key">x-api-key</option>
                  <option value="api_key">api-key</option>
                </select>
              </div>
            </div>
            <label className={labelCls}>{t("apiKeyStored")}</label>
            <input
              required
              type="password"
              value={provider.api_key}
              onChange={(e) => setProvider({ ...provider, api_key: e.target.value })}
              placeholder="sk-..."
              className={inputCls}
            />
            <label className={labelCls}>{t("model")}</label>
            <div className="grid grid-cols-2 gap-2">
              <input
                value={provider.models[0].local}
                onChange={(e) => {
                  const next = [...provider.models];
                  next[0] = { ...next[0], local: e.target.value };
                  setProvider({ ...provider, models: next });
                }}
                placeholder={t("localName")}
                className={inputCls}
              />
              <input
                value={provider.models[0].remote}
                onChange={(e) => {
                  const next = [...provider.models];
                  next[0] = { ...next[0], remote: e.target.value };
                  setProvider({ ...provider, models: next });
                }}
                placeholder={t("providerName")}
                className={inputCls}
              />
            </div>
            <div className="grid grid-cols-3 gap-2">
              <div>
                <label className={labelCls}>{t("inputCost")}</label>
                <input
                  type="number"
                  step="0.0001"
                  value={provider.models[0].input_cost_per_1k ?? ""}
                  onChange={(e) => {
                    const next = [...provider.models];
                    next[0] = {
                      ...next[0],
                      input_cost_per_1k: e.target.value ? Number(e.target.value) : null,
                    };
                    setProvider({ ...provider, models: next });
                  }}
                  className={inputCls}
                />
              </div>
              <div>
                <label className={labelCls}>{t("outputCost")}</label>
                <input
                  type="number"
                  step="0.0001"
                  value={provider.models[0].output_cost_per_1k ?? ""}
                  onChange={(e) => {
                    const next = [...provider.models];
                    next[0] = {
                      ...next[0],
                      output_cost_per_1k: e.target.value ? Number(e.target.value) : null,
                    };
                    setProvider({ ...provider, models: next });
                  }}
                  className={inputCls}
                />
              </div>
              <div>
                <label className={labelCls}>{t("cachedInputCost")}</label>
                <input
                  type="number"
                  step="0.0001"
                  value={provider.models[0].cached_input_cost_per_1k ?? ""}
                  onChange={(e) => {
                    const next = [...provider.models];
                    next[0] = {
                      ...next[0],
                      cached_input_cost_per_1k: e.target.value
                        ? Number(e.target.value)
                        : null,
                    };
                    setProvider({ ...provider, models: next });
                  }}
                  className={inputCls}
                />
              </div>
            </div>
            {error && <p className="text-xs text-red-400">{error}</p>}
            <div className="flex justify-between pt-2">
              <button
                type="button"
                onClick={onComplete}
                className="text-xs text-neutral-500 hover:text-neutral-300"
              >
                {t("skipOnboarding")}
              </button>
              <button
                type="submit"
                className="rounded-md bg-emerald-500/20 px-4 py-1.5 text-xs font-semibold text-emerald-700 hover:bg-emerald-500/30 dark:text-emerald-300"
              >
                {t("next")}
              </button>
            </div>
          </form>
        )}

        {step === "project" && (
          <form onSubmit={submitProject} className="space-y-3">
            <div>
              <h2 className="text-base font-semibold">{t("onboardingProjectTitle")}</h2>
              <p className="text-xs text-neutral-500">{t("onboardingProjectSubtitle")}</p>
            </div>
            <label className={labelCls}>{t("projectName")}</label>
            <input
              required
              value={project.name}
              onChange={(e) => setProject({ ...project, name: e.target.value })}
              className={inputCls}
            />
            <label className={labelCls}>{t("labelKey")}</label>
              <div className="flex gap-2">
                <input
                  required
                  value={project.label_key}
                  onChange={(e) => setProject({ ...project, label_key: e.target.value })}
                  className={inputCls}
                />
                <button
                  type="button"
                  onClick={() => setProject({ ...project, label_key: randomLabelKey() })}
                  className="whitespace-nowrap rounded-md bg-neutral-200 px-3 py-1.5 text-xs text-neutral-700 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
                >
                  {t("regenerate")}
                </button>
              </div>
            {error && <p className="text-xs text-red-400">{error}</p>}
            <div className="flex justify-between pt-2">
              <button
                type="button"
                onClick={() => setStep("provider")}
                className="text-xs text-neutral-500 hover:text-neutral-300"
              >
                {t("back")}
              </button>
              <button
                type="submit"
                className="rounded-md bg-emerald-500/20 px-4 py-1.5 text-xs font-semibold text-emerald-700 hover:bg-emerald-500/30 dark:text-emerald-300"
              >
                {t("next")}
              </button>
            </div>
          </form>
        )}

        {step === "connect" && (
          <div className="space-y-3">
            <div>
              <h2 className="text-base font-semibold">{t("onboardingConnectTitle")}</h2>
              <p className="text-xs text-neutral-500">{t("onboardingConnectSubtitle")}</p>
            </div>

            <div className="space-y-2">
              <div>
                <label className={labelCls}>{t("proxyEndpoint")}</label>
                <div className="flex gap-2">
                  <code className="flex-1 truncate rounded-md border border-neutral-300 bg-neutral-100 px-3 py-2 text-sm text-emerald-700 dark:border-neutral-700 dark:bg-neutral-950 dark:text-emerald-300">
                    {settings?.proxy_url ?? "http://127.0.0.1:3742"}
                  </code>
                  <button
                    type="button"
                    onClick={() => copy(settings?.proxy_url ?? "", setCopiedProxy)}
                    className="rounded-md bg-neutral-200 px-3 py-1.5 text-xs text-neutral-700 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
                  >
                    {copiedProxy ? t("copied") : t("copy")}
                  </button>
                </div>
              </div>

              <div>
                <label className={labelCls}>{t("labelKey")}</label>
                <div className="flex gap-2">
                  <code className="flex-1 truncate rounded-md border border-neutral-300 bg-neutral-100 px-3 py-2 text-sm text-emerald-700 dark:border-neutral-700 dark:bg-neutral-950 dark:text-emerald-300">
                    {project.label_key}
                  </code>
                  <button
                    type="button"
                    onClick={() => copy(project.label_key, setCopiedKey)}
                    className="rounded-md bg-neutral-200 px-3 py-1.5 text-xs text-neutral-700 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
                  >
                    {copiedKey ? t("copied") : t("copy")}
                  </button>
                </div>
              </div>
            </div>

            <div>
              <label className={labelCls}>{t("exampleRequest")}</label>
              <pre className="overflow-x-auto rounded-md bg-neutral-900 p-3 text-[10px] text-neutral-100 dark:bg-neutral-950">
                {`curl ${settings?.proxy_url ?? "http://127.0.0.1:3742"}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${project.label_key}" \\
  -d '{"model":"${provider.models[0].local}","messages":[{"role":"user","content":"hello"}]}'`}
              </pre>
            </div>

            <div className="flex justify-end pt-2">
              <button
                type="button"
                onClick={onComplete}
                className="rounded-md bg-emerald-500/20 px-4 py-1.5 text-xs font-semibold text-emerald-700 hover:bg-emerald-500/30 dark:text-emerald-300"
              >
                {t("finish")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
