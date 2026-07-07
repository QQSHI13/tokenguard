import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { clearLicenseKey, validateStoredKey } from "../utils/license";
import { useI18n } from "../i18n";

type BannerConfig = {
  enabled: boolean;
  interval_hours: number;
  title: string;
  body: string;
  cta_url: string;
  dismiss_duration_hours: number;
};

type BannerState = {
  lastShownAt?: number;
  lastDismissedAt?: number;
};

const STORAGE_KEY = "tokenguard-banner-state";

const banners: BannerConfig = {
  enabled: true,
  interval_hours: 48,
  dismiss_duration_hours: 24,
  title: "Support Token Guard",
  body: "Buy a license key to remove this banner.",
  cta_url: "https://tokenguard.pages.dev/buy.html",
};

function loadState(): BannerState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      return JSON.parse(raw);
    }
  } catch {
    // ignore corrupt state
  }
  return {};
}

export default function Banner({
  licensed,
  onLicenseChange,
}: {
  licensed: boolean;
  onLicenseChange: (licensed: boolean) => void;
}) {
  const { t } = useI18n();
  const [visible, setVisible] = useState(false);
  const [state, setState] = useState<BannerState>(loadState);

  const saveState = (next: BannerState) => {
    setState(next);
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
    } catch {
      // ignore write errors
    }
  };

  useEffect(() => {
    let cancelled = false;

    const check = async () => {
      if (!banners.enabled) {
        if (!cancelled) setVisible(false);
        return;
      }

      const now = Date.now();
      const { interval_hours, dismiss_duration_hours } = banners;

      if (
        state.lastDismissedAt &&
        now - state.lastDismissedAt < dismiss_duration_hours * 60 * 60 * 1000
      ) {
        if (!cancelled) setVisible(false);
        return;
      }

      if (
        state.lastShownAt &&
        now - state.lastShownAt < interval_hours * 60 * 60 * 1000
      ) {
        if (!cancelled) setVisible(false);
        return;
      }

      if (licensed) {
        const valid = await validateStoredKey();
        if (cancelled) return;

        if (valid) {
          setVisible(false);
        } else {
          await clearLicenseKey();
          onLicenseChange(false);
          const next = { ...state, lastShownAt: now };
          saveState(next);
          setVisible(true);
        }
      } else {
        const next = { ...state, lastShownAt: now };
        saveState(next);
        setVisible(true);
      }
    };

    check();

    return () => {
      cancelled = true;
    };
    // Intentionally depend only on `licensed` so state updates inside this
    // effect do not immediately re-trigger it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [licensed]);

  const handleDismiss = () => {
    saveState({ ...state, lastDismissedAt: Date.now() });
    setVisible(false);
  };

  const handleCta = () => {
    if (banners.cta_url) {
      openUrl(banners.cta_url).catch(console.error);
    }
  };

  if (!visible) return null;

  return (
    <div className="border-t border-neutral-200 bg-neutral-50 px-4 py-3 dark:border-neutral-800 dark:bg-neutral-900/60">
      <div className="flex items-start justify-between gap-4">
        <div className="flex-1">
          <h3 className="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
            {t("supportTokenGuard")}
          </h3>
          <p className="mt-1 text-xs text-neutral-600 dark:text-neutral-400">
            {t("buyLicenseKeyToRemoveBanner")}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button
            onClick={handleCta}
            className="rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-emerald-700"
          >
            {t("learnMore")}
          </button>
          <button
            onClick={handleDismiss}
            className="rounded-md bg-neutral-200 px-3 py-1.5 text-xs font-semibold text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
          >
            {t("dismiss")}
          </button>
        </div>
      </div>
    </div>
  );
}
