import { Monitor, Moon, Sun } from "lucide-react";
import { SlidingSegmentedControl } from "@/components/ui/sliding-segmented-control";
import { useTranslation } from "react-i18next";
import { useTheme } from "@/components/theme-provider";

export function ThemeSettings() {
  const { t } = useTranslation();
  const { theme, setTheme } = useTheme();
  const options = [
    {
      value: "light",
      label: (
        <>
          <Sun className="h-3.5 w-3.5" />
          {t("settings.themeLight")}
        </>
      ),
    },
    {
      value: "dark",
      label: (
        <>
          <Moon className="h-3.5 w-3.5" />
          {t("settings.themeDark")}
        </>
      ),
    },
    {
      value: "system",
      label: (
        <>
          <Monitor className="h-3.5 w-3.5" />
          {t("settings.themeSystem")}
        </>
      ),
    },
  ] as const;

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.theme")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.themeHint")}
        </p>
      </header>
      <SlidingSegmentedControl
        value={theme}
        onValueChange={(value) => setTheme(value)}
        options={options}
        ariaLabel={t("settings.theme")}
        buttonClassName="min-w-[96px] gap-1.5"
      />
    </section>
  );
}
