import { useEffect } from "react";
import { useTranslation } from "react-i18next";

const SUPPORTED_LANGUAGES = new Set(["en", "zh", "zh-TW", "ja"]);

type LanguageController = {
  language: string;
  changeLanguage: (language: string) => Promise<unknown>;
};

export async function syncLanguage(
  i18n: LanguageController,
  language?: string | null,
) {
  if (
    !language ||
    !SUPPORTED_LANGUAGES.has(language) ||
    i18n.language === language
  )
    return;
  await i18n.changeLanguage(language);
}

export function useLanguageSync(language?: string) {
  const { i18n } = useTranslation();
  useEffect(() => {
    void syncLanguage(i18n, language);
  }, [i18n, language]);
}
