import { SettingRow } from "./SettingsGroup";
import { SlidingSegmentedControl } from "@/components/ui/sliding-segmented-control";
import { useTranslation } from "react-i18next";
import { useTheme } from "@/components/theme-provider";

export function ThemeSettings() {
  const { t } = useTranslation();
  const { theme, setTheme } = useTheme();
  const options = [
    {
      value: "light",
      label: t("settings.themeLight"),
    },
    {
      value: "dark",
      label: t("settings.themeDark"),
    },
    {
      value: "system",
      label: t("settings.themeSystem"),
    },
  ] as const;

  return (
    <SettingRow
      title={t("settings.theme")}
      description={t("settings.themeHint")}
    >
      <SlidingSegmentedControl
        value={theme}
        onValueChange={(value) => setTheme(value)}
        options={options}
        indicatorVariant="surface"
        className="max-w-full"
        buttonClassName="h-[30px] px-2 text-xs"
        ariaLabel={t("settings.theme")}
      />
    </SettingRow>
  );
}
