import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { telemetryApi } from "@/lib/api/telemetry";

const preferencesKey = ["localTelemetryPreferences"] as const;

export function PrivacySettings() {
  const { t } = useTranslation();
  const client = useQueryClient();
  const preferences = useQuery({
    queryKey: preferencesKey,
    queryFn: telemetryApi.getPreferences,
    retry: false,
  });
  const update = useMutation({
    mutationFn: telemetryApi.setEnabled,
    onSuccess: (value) => client.setQueryData(preferencesKey, value),
  });
  const enabled = preferences.data?.enabled ?? false;
  const unavailable = !preferences.data?.available;

  return (
    <div className="space-y-3">
      <div className="flex items-start justify-between gap-6">
        <label htmlFor="optional-statistics" className="space-y-1">
          <span className="text-sm font-medium">{t("privacy.statistics")}</span>
          <p className="text-xs text-muted-foreground">
            {t("privacy.description")}
          </p>
        </label>
        <Switch
          id="optional-statistics"
          checked={enabled}
          onCheckedChange={(value) => update.mutate(value)}
          disabled={
            preferences.isPending ||
            preferences.isError ||
            update.isPending ||
            (unavailable && !enabled)
          }
        />
      </div>
      {unavailable && !preferences.isPending && !preferences.isError && (
        <p className="text-xs text-muted-foreground">
          {t("privacy.unavailable")}
        </p>
      )}
      {(preferences.isError || update.isError) && (
        <p role="alert" className="text-xs text-destructive">
          {t("privacy.failed")}
        </p>
      )}
    </div>
  );
}
