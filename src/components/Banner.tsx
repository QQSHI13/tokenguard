import { useEffect, useState } from "react";
import { validateStoredKey } from "../utils/license";

type BannersConfig = {
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
const FIVE_MINUTES = 5 * 60 * 1000;

const banners: BannersConfig = {
  enabled: true,
  interval_hours: 48,
  dismiss_duration_hours: 24,
  title: "Support Token Guard",
  body: "Buy a license key to remove this banner.",
  cta_url: "https://tokenguard.pages.dev/buy.html",
};

export default function Banner({
  licensed,
  onLicenseChange,
}: {
  licensed: boolean;
  onLicenseChange: (licensed: boolean) => void;
}) {
  const [state, setState] = useState<BannerState>({});

  useEffect(() => {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        setState(JSON.parse(raw));
      }
    } catch {
      // ignore corrupt state
    }
  }, []);

  useEffect(() => {
    const interval = setInterval(async () => {
      const valid = await validateStoredKey();
      if (!valid && licensed) {
        onLicenseChange(false);
      }
    }, FIVE_MINUTES);
    return () => clearInterval(interval);
  }, [licensed, onLicenseChange]);

  const saveState = (next: BannerState) => {
    setState(next);
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
    } catch {
      // ignore write errors
    }
  };

  const shouldShow = (): boolean => {
    if (!banners.enabled || licensed) return false;

    const now = Date.now();
    const { interval_hours, dismiss_duration_hours } = banners;

    if (
      state.lastDismissedAt &&
      now - state.lastDismissedAt < dismiss_duration_hours * 60 * 60 * 1000
    ) {
      return false;
    }

    if (
      state.lastShownAt &&
      now - state.lastShownAt < interval_hours * 60 * 60 * 1000
    ) {
      return false;
    }

    return true;
  };

  const visible = shouldShow();

  useEffect(() => {
    if (visible) {
      saveState({ ...state, lastShownAt: Date.now() });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible]);

  const handleDismiss = () => {
    saveState({ ...state, lastDismissedAt: Date.now() });
  };

  const handleCta = () => {
    if (banners.cta_url) {
      window.open(banners.cta_url, "_blank");
    }
  };

  if (!visible) return null;

  return (
    <div className="border-t border-neutral-200 bg-neutral-50 px-4 py-3 dark:border-neutral-800 dark:bg-neutral-900/60">
      <div className="flex items-start justify-between gap-4">
        <div className="flex-1">
          <h3 className="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
            {banners.title}
          </h3>
          <p className="mt-1 text-xs text-neutral-600 dark:text-neutral-400">
            {banners.body}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button
            onClick={handleCta}
            className="rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-emerald-700"
          >
            Learn more
          </button>
          <button
            onClick={handleDismiss}
            className="rounded-md bg-neutral-200 px-3 py-1.5 text-xs font-semibold text-neutral-800 hover:bg-neutral-300 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
          >
            Dismiss
          </button>
        </div>
      </div>
    </div>
  );
}
