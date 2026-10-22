import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { SecurityActivitySummary } from "@/components/security/SecurityActivitySummary";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { useSettings } from "@/hooks/useSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

interface StatisticsPageProps {
  onOpenSecurity?: () => void;
}

export function StatisticsPage({ onOpenSecurity }: StatisticsPageProps) {
  const { t } = useTranslation();
  const { settings, updateSettings, autoSaveSettings } = useSettings();

  const handleAutoSave = useCallback(
    async (updates: Partial<SettingsFormState>): Promise<boolean> => {
      if (!settings) return false;

      const previousValues = Object.fromEntries(
        Object.keys(updates).map((key) => [
          key,
          settings[key as keyof SettingsFormState],
        ]),
      ) as Partial<SettingsFormState>;

      updateSettings(updates);
      try {
        await autoSaveSettings(updates);
        return true;
      } catch (error) {
        console.error("[StatisticsPage] Failed to autosave settings", error);
        updateSettings(previousValues);
        toast.error(
          t("settings.saveFailedGeneric", {
            defaultValue: "保存失败，请重试",
          }),
        );
        return false;
      }
    },
    [autoSaveSettings, settings, t, updateSettings],
  );

  return (
    <div className="mx-auto flex h-full w-full max-w-7xl flex-col overflow-hidden px-6 pt-5">
      <div className="flex-1 overflow-y-auto overflow-x-hidden pr-2">
        <SecurityActivitySummary
          onOpenSecurity={onOpenSecurity ?? (() => undefined)}
        />
        <UsageDashboard
          refreshIntervalMs={settings?.usageDashboardRefreshIntervalMs}
          onRefreshIntervalChange={(refreshIntervalMs) =>
            handleAutoSave({
              usageDashboardRefreshIntervalMs: refreshIntervalMs,
            })
          }
          sessionAutoSyncEnabled={settings?.sessionAutoSyncEnabled ?? true}
          onSessionAutoSyncEnabledChange={(sessionAutoSyncEnabled) =>
            handleAutoSave({ sessionAutoSyncEnabled })
          }
        />
      </div>
    </div>
  );
}
