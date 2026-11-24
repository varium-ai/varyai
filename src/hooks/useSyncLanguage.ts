import { useEffect } from "react";
import { useTranslation } from "react-i18next";

const SUPPORTED_LANGUAGES = new Set(["en", "zh", "zh-TW", "ja"]);

/**
 * Syncs i18next's active language with the persisted settings language.
 */
export function useSyncLanguage(language?: string | null) {
  const { i18n } = useTranslation();

  useEffect(() => {
    const hasBundle =
      typeof i18n.hasResourceBundle !== "function" ||
      i18n.hasResourceBundle(language ?? "", "translation");
    if (
      language &&
      SUPPORTED_LANGUAGES.has(language) &&
      hasBundle &&
      i18n.language !== language
    ) {
      void i18n.changeLanguage(language);
    }
  }, [language, i18n]);

  return i18n;
}
