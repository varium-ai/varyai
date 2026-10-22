import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";
import type { SettingsFormState } from "@/hooks/useSettings";
import type { VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import { APP_ICON_NAME, DEFAULT_VISIBLE_APPS } from "@/config/appConfig";

interface AppVisibilitySettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

const APP_CONFIG: Array<{
  id: AppId;
  icon: string;
  nameKey: string;
}> = [
  { id: "claude", icon: "claude", nameKey: "apps.claudeCode" },
  {
    id: "claude-desktop",
    icon: "claude",
    nameKey: "apps.claudeDesktop",
  },
  { id: "codex", icon: "openai", nameKey: "apps.codex" },
  { id: "gemini", icon: "gemini", nameKey: "apps.gemini" },
  { id: "grokbuild", icon: "grok", nameKey: "apps.grokbuild" },
  { id: "opencode", icon: "opencode", nameKey: "apps.opencode" },
  { id: "openclaw", icon: "openclaw", nameKey: "apps.openclaw" },
  { id: "hermes", icon: "hermes", nameKey: "apps.hermes" },
  { id: "pi", icon: "pi", nameKey: "apps.pi" },
  { id: "workbuddy", icon: APP_ICON_NAME.workbuddy, nameKey: "apps.workbuddy" },
  { id: "qoder", icon: APP_ICON_NAME.qoder, nameKey: "apps.qoder" },
  {
    id: "doubaowork",
    icon: APP_ICON_NAME.doubaowork,
    nameKey: "apps.doubaowork",
  },
  { id: "trae", icon: APP_ICON_NAME.trae, nameKey: "apps.trae" },
  { id: "kimi", icon: APP_ICON_NAME.kimi, nameKey: "apps.kimi" },
  { id: "qwen", icon: APP_ICON_NAME.qwen, nameKey: "apps.qwen" },
  { id: "zcode", icon: APP_ICON_NAME.zcode, nameKey: "apps.zcode" },
];

export function AppVisibilitySettings({
  settings,
  onChange,
}: AppVisibilitySettingsProps) {
  const { t } = useTranslation();

  const visibleApps: VisibleApps = {
    ...DEFAULT_VISIBLE_APPS,
    ...settings.visibleApps,
  };

  // Count how many apps are currently visible
  const visibleCount = Object.values(visibleApps).filter(Boolean).length;

  const handleToggle = (appId: AppId) => {
    const isCurrentlyVisible = visibleApps[appId];
    // Prevent disabling the last visible app
    if (isCurrentlyVisible && visibleCount <= 1) return;

    onChange({
      visibleApps: {
        ...visibleApps,
        [appId]: !isCurrentlyVisible,
      },
    });
  };

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.appVisibility.title")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.appVisibility.description")}
        </p>
      </header>
      <div className="flex flex-wrap gap-3 rounded-xl border border-border-default bg-background p-4">
        {APP_CONFIG.map((app) => {
          const isVisible = visibleApps[app.id] ?? true;
          // Disable button if this is the last visible app
          const isDisabled = isVisible && visibleCount <= 1;

          return (
            <AppButton
              key={app.id}
              active={isVisible}
              disabled={isDisabled}
              onClick={() => handleToggle(app.id)}
              icon={app.icon}
              name={t(app.nameKey)}
            >
              {t(app.nameKey)}
            </AppButton>
          );
        })}
      </div>
    </section>
  );
}

interface AppButtonProps {
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
  icon: string;
  name: string;
  children: React.ReactNode;
}

function AppButton({
  active,
  disabled,
  onClick,
  icon,
  name,
  children,
}: AppButtonProps) {
  return (
    <Button
      type="button"
      onClick={onClick}
      disabled={disabled}
      size="sm"
      variant={active ? "default" : "ghost"}
      className={cn(
        "h-12 min-w-[120px] w-auto gap-2.5 rounded-xl px-5 text-sm",
        active
          ? "shadow-sm"
          : "text-muted-foreground hover:text-foreground hover:bg-muted",
      )}
    >
      <ProviderIcon icon={icon} name={name} size={18} />
      {children}
    </Button>
  );
}
