import { useTranslation } from "react-i18next";
import { Plus, X } from "lucide-react";
import { SettingSwitch } from "./SettingsGroup";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";
import type { SettingsFormState } from "@/hooks/useSettings";
import type { VisibleApps } from "@/types";
import type { AppId } from "@/lib/api";
import { APP_ICON_NAME, DEFAULT_VISIBLE_APPS } from "@/config/appConfig";

interface AppVisibilitySettingsProps {
  showAppChoices?: boolean;
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
  { id: "t3code", icon: "t3code", nameKey: "apps.t3code" },
  { id: "zcode", icon: APP_ICON_NAME.zcode, nameKey: "apps.zcode" },
  {
    id: "deepseek-harness",
    icon: "deepseek",
    nameKey: "apps.deepseek-harness",
  },
];

export function AppVisibilitySettings({
  settings,
  onChange,
  showAppChoices = true,
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
    <>
      {showAppChoices && (
        <div className="settings-detail space-y-3">
          <div className="setting-copy">
            <h3>{t("settings.visibleApplications")}</h3>
            <p>{t("settings.appVisibility.description")}</p>
          </div>
          {[true, false].map((active) => {
            const apps = APP_CONFIG.filter(
              ({ id }) => visibleApps[id] === active,
            );
            if (!apps.length) return null;
            return (
              <div key={String(active)} className="space-y-1.5">
                <p className="text-xs text-muted-foreground">
                  {active
                    ? t("settings.selectedApplications", {
                        count: visibleCount,
                      })
                    : t("settings.addApplication")}
                </p>
                <div className="flex flex-wrap gap-1.5">
                  {apps.map((app) => (
                    <AppButton
                      key={app.id}
                      active={active}
                      disabled={active && visibleCount <= 1}
                      onClick={() => handleToggle(app.id)}
                      icon={app.icon}
                      name={t(app.nameKey)}
                    >
                      {t(app.nameKey)}
                    </AppButton>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}
      <SettingSwitch
        title={t("settings.appVisibility.showProfileSwitcher")}
        description={t("settings.appVisibility.showProfileSwitcherDescription")}
        checked={settings.showProfileSwitcher ?? true}
        onCheckedChange={(value) => onChange({ showProfileSwitcher: value })}
      />
    </>
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
      variant="ghost"
      aria-pressed={active}
      className={cn(
        "h-7 gap-1.5 rounded-md px-2 text-xs font-medium",
        active
          ? "bg-accent text-secondary-foreground"
          : "border border-border bg-card text-muted-foreground",
      )}
    >
      {!active && <Plus aria-hidden="true" size={12} />}
      <ProviderIcon icon={icon} name={name} size={14} />
      {children}
      {active && <X aria-hidden="true" size={12} />}
    </Button>
  );
}
