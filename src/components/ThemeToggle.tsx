import { useEffect, useState } from "react";
import { useI18n } from "../i18n";

type Theme = "light" | "dark" | "system";

function getSystemTheme(): "light" | "dark" {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const effective = theme === "system" ? getSystemTheme() : theme;
  if (effective === "dark") {
    root.classList.add("dark");
  } else {
    root.classList.remove("dark");
  }
}

export function getStoredTheme(): Theme {
  try {
    const raw = localStorage.getItem("tokenguard-theme");
    if (raw === "light" || raw === "dark" || raw === "system") return raw;
  } catch {
    // ignore
  }
  return "dark";
}

export function storeTheme(theme: Theme) {
  try {
    localStorage.setItem("tokenguard-theme", theme);
  } catch {
    // ignore
  }
  applyTheme(theme);
}

export function initTheme() {
  applyTheme(getStoredTheme());
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  const handler = () => applyTheme(getStoredTheme());
  mq.addEventListener("change", handler);
  return () => mq.removeEventListener("change", handler);
}

export default function ThemeToggle() {
  const { t } = useI18n();
  const [theme, setTheme] = useState<Theme>(() => getStoredTheme());

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  const cycle = () => {
    const next: Theme = theme === "dark" ? "light" : theme === "light" ? "system" : "dark";
    setTheme(next);
    storeTheme(next);
  };

  const label = theme === "dark" ? t("dark") : theme === "light" ? t("light") : t("system");
  const isChecked = theme === "dark";

  return (
    <button
      onClick={cycle}
      title={`${t("theme")}: ${label}`}
      className="flex items-center gap-2 rounded-md px-2 py-1 text-xs hover:bg-neutral-200 dark:hover:bg-neutral-800"
      aria-pressed={isChecked}
    >
      <span
        className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
          isChecked ? "bg-emerald-500" : "bg-neutral-300 dark:bg-neutral-600"
        }`}
      >
        <span
          className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white transition-transform ${
            isChecked ? "translate-x-5" : "translate-x-1"
          }`}
        />
      </span>
      <span className="hidden text-neutral-600 dark:text-neutral-400 sm:inline">{label}</span>
    </button>
  );
}
