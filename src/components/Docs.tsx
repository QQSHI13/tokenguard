import { useI18n } from "../i18n";

const DOCS_LANG: Record<string, string> = {
  en: "en",
  "zh-CN": "zh",
};

export default function DocsTab() {
  const { lang } = useI18n();
  const docsLang = DOCS_LANG[lang] ?? "en";
  return (
    <div className="h-full w-full overflow-hidden rounded-lg border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900/40">
      <iframe
        title="Token Guard Docs"
        src={`./docs/${docsLang}/index.html`}
        className="h-full w-full border-0"
      />
    </div>
  );
}
