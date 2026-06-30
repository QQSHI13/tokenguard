import { useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useI18n } from "../i18n";

const DOCS_LANG: Record<string, string> = {
  en: "en",
  "zh-CN": "zh",
};

// Import all markdown source files for each book.
const enModules = import.meta.glob("../../docs/book-en/src/*.md", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;
const zhModules = import.meta.glob("../../docs/book-zh/src/*.md", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;

function modulesByFileName(modules: Record<string, string>) {
  return Object.fromEntries(
    Object.entries(modules).map(([path, content]) => {
      const name = path.split("/").pop() ?? path;
      return [name, content];
    })
  );
}

const enByName = modulesByFileName(enModules);
const zhByName = modulesByFileName(zhModules);

// SUMMARY.md files drive the table of contents.
import summaryEnRaw from "../../docs/book-en/src/SUMMARY.md?raw";
import summaryZhRaw from "../../docs/book-zh/src/SUMMARY.md?raw";

type SummaryItem = { title: string; file: string };

function parseSummary(raw: string): SummaryItem[] {
  const items: SummaryItem[] = [];
  for (const line of raw.split("\n")) {
    const match = line.match(/^-\s*\[(.+?)\]\((.+?)\)/);
    if (match) {
      items.push({ title: match[1], file: match[2] });
    }
  }
  return items;
}

const summaries: Record<string, SummaryItem[]> = {
  en: parseSummary(summaryEnRaw),
  "zh-CN": parseSummary(summaryZhRaw),
};

export default function DocsTab({ onClose }: { onClose: () => void }) {
  const { lang, t } = useI18n();
  const docsLang = DOCS_LANG[lang] ?? "en";
  const summary = summaries[lang] ?? summaries.en;
  const byName = docsLang === "zh" ? zhByName : enByName;

  const [selected, setSelected] = useState<string>(summary[0]?.file ?? "");

  const content = useMemo(() => {
    if (!selected) return "";
    return byName[selected] ?? `*${t("noDataToChart")}*`;
  }, [byName, selected, t]);

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

      <div className="flex flex-1 overflow-hidden">
        {/* Table of contents */}
        <aside className="w-48 overflow-y-auto border-r border-neutral-200 bg-neutral-50 px-2 py-3 dark:border-neutral-800 dark:bg-neutral-950/40">
          <nav className="space-y-1">
            {summary.map((item) => (
              <button
                key={item.file}
                type="button"
                onClick={() => setSelected(item.file)}
                className={`w-full rounded-md px-2 py-1.5 text-left text-xs transition-colors ${
                  selected === item.file
                    ? "bg-emerald-500/10 font-medium text-emerald-700 dark:text-emerald-300"
                    : "text-neutral-600 hover:bg-neutral-200 dark:text-neutral-400 dark:hover:bg-neutral-800"
                }`}
              >
                {item.title}
              </button>
            ))}
          </nav>
        </aside>

        {/* Content */}
        <main className="flex-1 overflow-y-auto p-4 text-sm text-neutral-900 dark:text-neutral-100">
          <article className="max-w-2xl">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                h1: ({ children }) => (
                  <h1 className="mb-3 text-xl font-semibold">{children}</h1>
                ),
                h2: ({ children }) => (
                  <h2 className="mb-2 mt-5 text-lg font-semibold">{children}</h2>
                ),
                h3: ({ children }) => (
                  <h3 className="mb-2 mt-4 text-base font-semibold">{children}</h3>
                ),
                p: ({ children }) => (
                  <p className="mb-3 leading-relaxed text-neutral-700 dark:text-neutral-300">
                    {children}
                  </p>
                ),
                a: ({ href, children }) => (
                  <a
                    href={href}
                    target="_blank"
                    rel="noreferrer"
                    className="text-emerald-600 underline hover:text-emerald-700 dark:text-emerald-400 dark:hover:text-emerald-300"
                  >
                    {children}
                  </a>
                ),
                ul: ({ children }) => (
                  <ul className="mb-3 list-disc space-y-1 pl-5">{children}</ul>
                ),
                ol: ({ children }) => (
                  <ol className="mb-3 list-decimal space-y-1 pl-5">{children}</ol>
                ),
                li: ({ children }) => (
                  <li className="text-neutral-700 dark:text-neutral-300">{children}</li>
                ),
                code: ({ children }) => (
                  <code className="rounded bg-neutral-100 px-1 py-0.5 font-mono text-xs text-neutral-800 dark:bg-neutral-800 dark:text-neutral-200">
                    {children}
                  </code>
                ),
                pre: ({ children }) => (
                  <pre className="mb-3 overflow-x-auto rounded-md bg-neutral-900 p-3 text-xs text-neutral-100 dark:bg-neutral-950">
                    {children}
                  </pre>
                ),
                blockquote: ({ children }) => (
                  <blockquote className="mb-3 border-l-4 border-emerald-500 pl-3 italic text-neutral-600 dark:text-neutral-400">
                    {children}
                  </blockquote>
                ),
                hr: () => <hr className="my-4 border-neutral-200 dark:border-neutral-800" />,
                table: ({ children }) => (
                  <table className="mb-3 w-full border-collapse text-left text-xs">
                    {children}
                  </table>
                ),
                th: ({ children }) => (
                  <th className="border-b border-neutral-300 px-2 py-1 font-semibold dark:border-neutral-700">
                    {children}
                  </th>
                ),
                td: ({ children }) => (
                  <td className="border-b border-neutral-200 px-2 py-1 dark:border-neutral-800">
                    {children}
                  </td>
                ),
              }}
            >
              {content}
            </ReactMarkdown>
          </article>
        </main>
      </div>
    </div>
  );
}
