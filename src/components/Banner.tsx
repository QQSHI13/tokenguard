import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { validateStoredKey, clearLicenseKey } from "../utils/license";

type Edition = "github-free" | "paid" | "microsoft-store";

type RuntimeConfig = {
  edition: Edition;
  banners: {
    enabled: boolean;
    interval_hours: number;
    title: string;
    body: string;
    cta_url: string;
    dismiss_duration_hours: number;
  };
};

type BannerState = {
  lastShownAt: number;
  lastDismissedAt: number;
};

const STORAGE_KEY = "tokenguard-banner-state";
const VALIDATION_INTERVAL_MS = 5 * 60 * 1000; // 5 minutes

function loadState(): BannerState | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    return JSON.parse(raw) as BannerState;
  } catch {
    return null;
  }
}

function saveState(state: BannerState) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // ignore
  }
}

function hours(ms: number) {
  return ms / 1000 / 60 / 60;
}

export default function Banner({
  licensed,
  onLicenseChange,
}: {
  licensed: boolean;
  onLicenseChange: (licensed: boolean) => void;
}) {
  const [config, setConfig] = useState<RuntimeConfig | null>(null);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    invoke<RuntimeConfig>("get_runtime_config")
      .then(setConfig)
      .catch(console.error);
  }, []);

  // Periodically revalidate the license key while the app is running.
  useEffect(() => {
    if (!licensed) return;
    const validate = async () => {
      const valid = await validateStoredKey();
      if (!valid) {
        await clearLicenseKey();
        onLicenseChange(false);
      }
    };
    validate();
    const i = setInterval(validate, VALIDATION_INTERVAL_MS);
    return () => clearInterval(i);
  }, [licensed, onLicenseChange]);

  useEffect(() => {
    if (!config || licensed) return;

    const { edition, banners } = config;
    if (edition !== "github-free" || !banners.enabled) return;

    const now = Date.now();
    const state = loadState();

    const sinceDismissed = state
      ? hours(now - state.lastDismissedAt)
      : Number.POSITIVE_INFINITY;
    const sinceShown = state
      ? hours(now - state.lastShownAt)
      : Number.POSITIVE_INFINITY;

    const shouldShow =
      sinceDismissed >= banners.dismiss_duration_hours &&
      sinceShown >= banners.interval_hours;

    if (shouldShow) {
      setVisible(true);
      saveState({
        lastShownAt: now,
        lastDismissedAt: state?.lastDismissedAt ?? 0,
      });
    }
  }, [config, licensed]);

  if (!visible || !config) return null;

  const { title, body, cta_url } = config.banners;

  const dismiss = () => {
    setVisible(false);
    saveState({
      lastShownAt: loadState()?.lastShownAt ?? Date.now(),
      lastDismissedAt: Date.now(),
    });
  };

  return (
    <div className="flex shrink-0 items-start gap-3 border-t border-indigo-100 bg-indigo-50 px-4 py-3 dark:border-indigo-950 dark:bg-indigo-950/40">
      <div className="flex-1">
        {title && (
          <p className="text-xs font-semibold text-indigo-900 dark:text-indigo-200">
            {title}
          </p>
        )}
        {body && (
          <p className="mt-0.5 text-xs text-indigo-800 dark:text-indigo-300">
            {body}
          </p>
        )}
      </div>
      {cta_url && (
        <button
          onClick={() => openUrl(cta_url).catch(console.error)}
          className="shrink-0 rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-700"
        >
          Learn more
        </button>
      )}
      <button
        onClick={dismiss}
        aria-label="Dismiss banner"
        className="shrink-0 text-indigo-600 hover:text-indigo-800 dark:text-indigo-300 dark:hover:text-indigo-100"
      >
        ✕
      </button>
    </div>
  );
}
