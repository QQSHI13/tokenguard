import { useI18n } from "../i18n";

const DOCS_LANG: Record<string, string> = {
  en: "en",
  "zh-CN": "zh",
};

export default function DocsTab({ onClose }: { onClose: () => void }) {
  const { lang, t } = useI18n();
  const docsLang = DOCS_LANG[lang] ?? "en";
  return (
    <div className="flex h-full flex-col overflow-hidden rounded-lg border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900/40">
      <div className="flex items-center justify-between border-b border-neutral-200 px-3 py-2 dark:border-neutral-800">
        <span className="text-sm font-semibold">{t("docs")}</span>
        <button
          type="button"
          onClick={onClose}
          className="rounded-md px-2 py-1 text-xs font-medium text-neutral-600 hover:bg-neutral-100 dark:text-neutral-300 dark:hover:bg-neutral-800"
        >
          ← {t("backToDashboard")}
        </button>
      </div>
      <iframe
        title="Token Guard Docs"
        src={`./docs/${docsLang}/index.html`}
        className="flex-1 border-0"
      />
    </div>
  );
}
