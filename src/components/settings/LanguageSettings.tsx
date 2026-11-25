import { SettingRow } from "./SettingsGroup";
import { SlidingSegmentedControl } from "@/components/ui/sliding-segmented-control";
import { useTranslation } from "react-i18next";

type LanguageOption = "zh" | "zh-TW" | "en" | "ja";

interface LanguageSettingsProps {
  value: LanguageOption;
  onChange: (value: LanguageOption) => void;
}

export function LanguageSettings({ value, onChange }: LanguageSettingsProps) {
  const { t } = useTranslation();
  const options = [
    { value: "zh", label: t("settings.languageOptionChinese") },
    {
      value: "zh-TW",
      label: t("settings.languageOptionTraditionalChinese"),
    },
    { value: "en", label: t("settings.languageOptionEnglish") },
    { value: "ja", label: t("settings.languageOptionJapanese") },
  ] as const;

  return (
    <SettingRow
      title={t("settings.language")}
      description={t("settings.languageHint")}
    >
      <SlidingSegmentedControl
        value={value}
        onValueChange={onChange}
        options={options}
        indicatorVariant="surface"
        className="max-w-full"
        buttonClassName="h-[30px] px-2 text-xs"
        ariaLabel={t("settings.language")}
        indicatorTestId="language-settings-indicator"
      />
    </SettingRow>
  );
}
