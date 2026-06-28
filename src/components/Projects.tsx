import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

type Project = { id: number; name: string; label_key: string };

function genKey(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return `tg_${hex}`;
}

export default function Projects({ onChange }: { onChange: () => void }) {
  const { t } = useI18n();
  const [projects, setProjects] = useState<Project[]>([]);
  const [name, setName] = useState("");
  const [labelKey, setLabelKey] = useState(genKey());
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<number | null>(null);

  const refresh = useCallback(() => {
    invoke<Project[]>("list_projects").then(setProjects).catch(console.error);
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await invoke("add_project", { input: { name: name.trim(), label_key: labelKey } });
      setName("");
      setLabelKey(genKey());
      refresh();
      onChange();
    } catch (err) {
      setError(String(err));
    }
  };

  const remove = async (id: number, name: string) => {
    if (!confirm(t("deleteProjectConfirm").replace("{name}", name))) return;
    await invoke("delete_project", { id });
    refresh();
    onChange();
  };

  const copy = async (p: Project) => {
    await navigator.clipboard.writeText(p.label_key);
    setCopied(p.id);
    setTimeout(() => setCopied(null), 1500);
  };

  return (
    <div className="max-w-2xl space-y-5 text-neutral-900 dark:text-neutral-100">
      <section className="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40">
        <h2 className="text-sm font-semibold">{t("howProjectTaggingWorks")}</h2>
        <p className="mt-1 text-[11px] leading-relaxed text-neutral-500">
          {t("projectTaggingHelp")}
        </p>
      </section>

      <div>
        <h2 className="mb-2 text-sm font-semibold">{t("projectsList")}</h2>
        <div className="space-y-2">
          {projects.length === 0 && (
            <p className="text-xs text-neutral-600">
              {t("noProjectsYet")}
            </p>
          )}
          {projects.map((p) => (
            <div
              key={p.id}
              className="flex items-center justify-between rounded-lg border border-neutral-200 bg-white px-3 py-2 dark:border-neutral-800 dark:bg-neutral-900/40"
            >
              <div className="min-w-0">
                <div className="text-sm font-medium">{p.name}</div>
                <div className="mt-0.5 flex items-center gap-2">
                  <code className="truncate font-mono text-[11px] text-sky-700 dark:text-sky-300">
                    {p.label_key}
                  </code>
                  <button
                    onClick={() => copy(p)}
                    className="text-[10px] text-neutral-500 hover:text-neutral-700 dark:hover:text-neutral-300"
                  >
                    {copied === p.id ? t("copied") : t("copy")}
                  </button>
                </div>
              </div>
              <button
                onClick={() => remove(p.id, p.name)}
                className="text-neutral-500 hover:text-red-400"
              >
                ✕
              </button>
            </div>
          ))}
        </div>
      </div>

      <form
        onSubmit={submit}
        className="space-y-3 rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/40"
      >
        <h2 className="text-sm font-semibold">{t("addProject")}</h2>
        <label className="block">
          <span className="mb-1 block text-[11px] text-neutral-500">{t("projectName")}</span>
          <input
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="cursor-app"
            className={inputCls}
          />
        </label>
        <label className="block">
          <span className="mb-1 block text-[11px] text-neutral-500">{t("labelKey")}</span>
          <div className="flex gap-2">
            <input
              required
              value={labelKey}
              onChange={(e) => setLabelKey(e.target.value)}
              className={`${inputCls} flex-1 font-mono text-sky-700 dark:text-sky-300`}
            />
            <button
              type="button"
              onClick={() => setLabelKey(genKey())}
              className="rounded-md bg-neutral-200 px-3 py-1.5 text-xs text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
            >
              {t("regenerate")}
            </button>
          </div>
        </label>
        {error && <p className="text-xs text-red-600 dark:text-red-400">{error}</p>}
        <button
          type="submit"
          className="w-full rounded-md bg-emerald-500/20 py-2 text-xs font-semibold text-emerald-700 hover:bg-emerald-500/30 dark:text-emerald-300"
        >
          {t("addProject")}
        </button>
      </form>
    </div>
  );
}

const inputCls =
  "w-full rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 focus:border-emerald-500 focus:outline-none dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200";
