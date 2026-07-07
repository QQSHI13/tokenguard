import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
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
  status: number | null;
};

type LogResult = {
  rows: Log[];
  total: number;
  page: number;
  page_size: number;
};

const PAGE_SIZE = 50;

export default function Logs() {
  const { t } = useI18n();
  const [rows, setRows] = useState<Log[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [provider, setProvider] = useState("");
  const [model, setModel] = useState("");
  const [project, setProject] = useState("");
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");
  const [providers, setProviders] = useState<{ provider: { name: string } }[]>([]);
  const [projects, setProjects] = useState<{ name: string }[]>([]);

  const refresh = useCallback(() => {
    const filter: Record<string, unknown> = {
      page,
      page_size: PAGE_SIZE,
    };
    if (provider) filter.provider = provider;
    if (model) filter.model = model;
    if (project) filter.project = project;
    if (start) filter.start = `${start}T00:00:00Z`;
    if (end) filter.end = `${end}T23:59:59Z`;
    invoke<LogResult>("get_logs_filtered", { filter })
      .then((r) => {
        setRows(r.rows);
        setTotal(r.total);
      })
      .catch(console.error);
  }, [page, provider, model, project, start, end]);

  useEffect(() => {
    refresh();
    const i = setInterval(refresh, 4000);
    return () => clearInterval(i);
  }, [refresh]);

  useEffect(() => {
    invoke<{ provider: { name: string } }[]>("list_providers")
      .then(setProviders)
      .catch(console.error);
    invoke<{ name: string }[]>("list_projects").then(setProjects).catch(console.error);
  }, []);

  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE));

  const statusClass = (s: number | null) => {
    if (s === null) return "text-neutral-500";
    if (s >= 200 && s < 300) return "text-emerald-600 dark:text-emerald-400";
    if (s === 429) return "text-amber-600 dark:text-amber-400";
    if (s >= 400) return "text-red-600 dark:text-red-400";
    return "text-neutral-500";
  };

  return (
    <div className="space-y-4 text-neutral-900 dark:text-neutral-100">
      <div className="flex flex-wrap gap-2">
        <select
          value={provider}
          onChange={(e) => {
            setProvider(e.target.value);
            setPage(1);
          }}
          className="rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
        >
          <option value="">{t("provider")}</option>
          {providers.map((p) => (
            <option key={p.provider.name} value={p.provider.name}>
              {p.provider.name}
            </option>
          ))}
        </select>
        <input
          type="text"
          value={model}
          onChange={(e) => {
            setModel(e.target.value);
            setPage(1);
          }}
          placeholder={t("model")}
          className="rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
        />
        <select
          value={project}
          onChange={(e) => {
            setProject(e.target.value);
            setPage(1);
          }}
          className="rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
        >
          <option value="">{t("project")}</option>
          {projects.map((p) => (
            <option key={p.name} value={p.name}>
              {p.name}
            </option>
          ))}
        </select>
        <input
          type="date"
          value={start}
          onChange={(e) => {
            setStart(e.target.value);
            setPage(1);
          }}
          className="rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
        />
        <input
          type="date"
          value={end}
          onChange={(e) => {
            setEnd(e.target.value);
            setPage(1);
          }}
          className="rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-xs text-neutral-900 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-200"
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
              <th className="px-3 py-2 text-center font-medium">{t("logStatus")}</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-neutral-200 dark:divide-neutral-800">
            {rows.length === 0 && (
              <tr>
                <td colSpan={8} className="px-3 py-6 text-center text-neutral-600">
                  {t("noLogsFound")}
                </td>
              </tr>
            )}
            {rows.map((l) => (
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
                <td className={`px-3 py-1.5 text-center font-medium ${statusClass(l.status)}`}>
                  {l.status ?? "—"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {totalPages > 1 && (
        <div className="flex items-center justify-between text-xs">
          <button
            onClick={() => setPage((p) => Math.max(1, p - 1))}
            disabled={page <= 1}
            className="rounded-md bg-neutral-200 px-3 py-1.5 text-neutral-800 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-200"
          >
            {t("previousPage")}
          </button>
          <span className="text-neutral-500">
            {t("pageNumber")} {page} {t("pageOf")} {totalPages}
          </span>
          <button
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            disabled={page >= totalPages}
            className="rounded-md bg-neutral-200 px-3 py-1.5 text-neutral-800 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-200"
          >
            {t("nextPage")}
          </button>
        </div>
      )}
    </div>
  );
}
