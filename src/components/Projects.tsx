import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

type Project = { id: number; name: string; label_key: string };

function genKey(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return `tg_${hex}`;
}

export default function Projects({ onChange }: { onChange: () => void }) {
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
    if (!confirm(`Delete project "${name}"?`)) return;
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
    <div className="max-w-2xl space-y-5">
      <section className="rounded-lg border border-neutral-800 bg-neutral-900/40 p-4">
        <h2 className="text-sm font-semibold text-neutral-200">How project tagging works</h2>
        <p className="mt-1 text-[11px] leading-relaxed text-neutral-500">
          Each project has a throwaway <em>label key</em>. Set it as{" "}
          <code className="text-neutral-400">OPENAI_API_KEY</code> (or{" "}
          <code className="text-neutral-400">x-api-key</code> for Anthropic clients) in your
          coding agent, plus <code className="text-neutral-400">OPENAI_BASE_URL=http://localhost:3742</code>.
          Token Guard tags that agent's requests with the project name and forwards using your
          real key from the keychain. No custom headers, no real keys in your agent config.
        </p>
      </section>

      <div>
        <h2 className="mb-2 text-sm font-semibold text-neutral-200">Projects</h2>
        <div className="space-y-2">
          {projects.length === 0 && (
            <p className="text-xs text-neutral-600">
              No projects yet. Without one, requests are tagged with no project.
            </p>
          )}
          {projects.map((p) => (
            <div
              key={p.id}
              className="flex items-center justify-between rounded-lg border border-neutral-800 bg-neutral-900/40 px-3 py-2"
            >
              <div className="min-w-0">
                <div className="text-sm font-medium text-neutral-200">{p.name}</div>
                <div className="mt-0.5 flex items-center gap-2">
                  <code className="truncate font-mono text-[11px] text-sky-300">
                    {p.label_key}
                  </code>
                  <button
                    onClick={() => copy(p)}
                    className="text-[10px] text-neutral-500 hover:text-neutral-300"
                  >
                    {copied === p.id ? "copied" : "copy"}
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
        className="space-y-3 rounded-lg border border-neutral-800 bg-neutral-900/40 p-4"
      >
        <h2 className="text-sm font-semibold text-neutral-200">Add project</h2>
        <label className="block">
          <span className="mb-1 block text-[11px] text-neutral-500">Project name</span>
          <input
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="cursor-app"
            className="w-full rounded-md border border-neutral-700 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-200 focus:border-emerald-500 focus:outline-none"
          />
        </label>
        <label className="block">
          <span className="mb-1 block text-[11px] text-neutral-500">Label key (set this in your agent)</span>
          <div className="flex gap-2">
            <input
              required
              value={labelKey}
              onChange={(e) => setLabelKey(e.target.value)}
              className="flex-1 rounded-md border border-neutral-700 bg-neutral-950 px-2.5 py-1.5 font-mono text-xs text-sky-300 focus:border-emerald-500 focus:outline-none"
            />
            <button
              type="button"
              onClick={() => setLabelKey(genKey())}
              className="rounded-md bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:bg-neutral-700"
            >
              Regenerate
            </button>
          </div>
        </label>
        {error && <p className="text-xs text-red-400">{error}</p>}
        <button
          type="submit"
          className="w-full rounded-md bg-emerald-500/20 py-2 text-xs font-semibold text-emerald-300 hover:bg-emerald-500/30"
        >
          Add project
        </button>
      </form>
    </div>
  );
}
