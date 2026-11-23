import "./statistics-page.css";
import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
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
    <div className="stats-page">
      <header className="stats-header">
        <div>
          <h1 className="vary-page-title">{t("usage.title")}</h1>
          <p>{t("usage.subtitle")}</p>
        </div>
        {onOpenSecurity && (
          <Button variant="outline" onClick={onOpenSecurity} type="button">
            {t("securityStats.viewLogs")}
          </Button>
        )}
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
