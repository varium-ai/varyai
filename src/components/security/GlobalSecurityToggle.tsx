import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { securityApi } from "@/lib/api/security";
import { securityKeys } from "@/hooks/useSecurityAudit";
import { Switch } from "@/components/ui/switch";
export function GlobalSecurityToggle() {
  const { t } = useTranslation();
  const client = useQueryClient();
  const state = useQuery({
    queryKey: securityKeys.settings(),
    queryFn: securityApi.getSettings,
  });
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  return (
    <div className="space-y-2">
      <label className="flex items-center gap-3 text-sm">
        {t("homeAudit.label")}
        <Switch
          checked={state.data?.enabled ?? false}
          disabled={!state.data || state.isError || busy}
          onCheckedChange={(enabled) => {
            if (!state.data) return;
            setBusy(true);
            setFailed(false);
            void securityApi
              .updateSettings({ ...state.data, enabled })
              .then(() =>
                client.invalidateQueries({ queryKey: securityKeys.settings() }),
              )
              .catch(() => setFailed(true))
              .finally(() => setBusy(false));
          }}
        />
      </label>
      {failed && (
        <p className="text-xs text-destructive" role="alert">
          {t("security.settingsUpdateFailed")}
        </p>
      )}
    </div>
  );
}
