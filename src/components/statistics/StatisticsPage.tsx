import "./statistics-page.css";
import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { WorkspacePageHeading } from "@/components/layout/WorkspacePageHeading";
import { toast } from "sonner";
import { SecurityActivitySummary } from "@/components/security/SecurityActivitySummary";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { useSettings } from "@/hooks/useSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

export function StatisticsPage() {
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
    <div className="stats-page">
      <header className="stats-header">
        <WorkspacePageHeading
          title={t("usage.title")}
          description={t("usage.subtitle")}
        />
      </header>
      <SecurityActivitySummary />
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
  );
}
