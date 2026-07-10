import { useState } from "react";
import { useI18n } from "../i18n";
import Providers from "./Providers";
import Limits from "./Limits";
import Projects from "./Projects";

type ConfigTab = "providers" | "limits" | "projects";

export default function Config({ onChange }: { onChange: () => void }) {
  const { t } = useI18n();
  const [tab, setTab] = useState<ConfigTab>("providers");

  const tabs: { id: ConfigTab; label: string }[] = [
    { id: "providers", label: t("providers") },
    { id: "limits", label: t("limits") },
    { id: "projects", label: t("projects") },
  ];

  return (
    <div className="space-y-4 text-neutral-900 dark:text-neutral-100">
      <div className="flex flex-wrap gap-1 border-b border-neutral-200 pb-1 dark:border-neutral-800">
        {tabs.map((tb) => (
          <button
            key={tb.id}
            onClick={() => setTab(tb.id)}
            className={`rounded-md px-3 py-1.5 text-xs font-medium transition-colors ${
              tab === tb.id
                ? "bg-emerald-500/10 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300"
                : "text-neutral-600 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-900/60"
            }`}
          >
            {tb.label}
          </button>
        ))}
      </div>

      {tab === "providers" && <Providers onChange={onChange} />}
      {tab === "limits" && <Limits onChange={onChange} />}
      {tab === "projects" && <Projects onChange={onChange} />}
    </div>
  );
}
