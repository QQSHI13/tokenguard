import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { clearLicenseKey, validateStoredKey } from "../utils/license";
import { useI18n } from "../i18n";

const CTA_URL = "https://tokenguard.pages.dev/buy.html";

export default function Banner({
  licensed,
  onLicenseChange,
}: {
  licensed: boolean;
  onLicenseChange: (licensed: boolean) => void;
}) {
  const { t } = useI18n();
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    let cancelled = false;

    const check = async () => {
      if (licensed) {
        const valid = await validateStoredKey();
        if (cancelled) return;
        if (!valid) {
          await clearLicenseKey();
          onLicenseChange(false);
        }
        setVisible(!valid);
      } else {
        setVisible(true);
      }
    };

    check();
    const i = setInterval(check, 1000 * 60 * 60); // revalidate hourly
    return () => {
      cancelled = true;
      clearInterval(i);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [licensed]);

  const handleCta = () => {
    openUrl(CTA_URL).catch(console.error);
  };

  if (!visible) return null;

  return (
    <div className="border-t border-neutral-200 bg-neutral-50 p-3 dark:border-neutral-800 dark:bg-neutral-900/60">
      <div className="space-y-2">
        <div>
          <h3 className="text-xs font-semibold text-neutral-900 dark:text-neutral-100">
            {t("supportTokenGuard")}
          </h3>
          <p className="mt-0.5 text-[11px] leading-snug text-neutral-600 dark:text-neutral-400">
            {t("buyLicenseKeyToRemoveBanner")}
          </p>
          <ul className="mt-2 list-inside list-disc space-y-0.5 text-[10px] text-neutral-500 dark:text-neutral-400">
            <li>{t("bannerFreeFeatureHistory")}</li>
            <li>{t("bannerFreeFeatureUpdates")}</li>
            <li>{t("bannerLicensedFeatureUnlimited")}</li>
            <li>{t("bannerLicensedFeatureUpdates")}</li>
          </ul>
          <p className="mt-2 text-[10px] font-medium text-emerald-700 dark:text-emerald-300">
            {t("bannerPriceNote")}
          </p>
        </div>
        <button
          onClick={handleCta}
          className="w-full rounded-md bg-emerald-600 px-2 py-1.5 text-[11px] font-semibold text-white hover:bg-emerald-700"
        >
          {t("learnMore")}
        </button>
      </div>
    </div>
  );
}
