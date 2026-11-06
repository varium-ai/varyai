import { lazy, Suspense, useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useSettingsQuery } from "@/lib/query";
import { settingsApi } from "@/lib/api";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";

const OnboardingWizard = lazy(() =>
  import("@/components/onboarding/OnboardingWizard").then((module) => ({
    default: module.OnboardingWizard,
  })),
);

/** Reuse the existing acknowledgement flag without changing stored settings. */
export function FirstRunNoticeDialog({
  onConnected,
}: {
  onConnected?: () => void;
}) {
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();

  const records = useQuery({
    queryKey: ["routing-upstreams"],
    queryFn: routingUpstreamsApi.list,
    enabled: settings != null && settings.firstRunNoticeConfirmed !== true,
  });
  const [automatic, setAutomatic] = useState<boolean | null>(null);
  useEffect(() => {
    if (automatic !== null || !settings) return;
    if (settings.firstRunNoticeConfirmed === true) setAutomatic(false);
    else if (records.isSuccess) setAutomatic(records.data.length === 0);
  }, [automatic, settings, records.isSuccess, records.data]);

  const handleFinish = async (connected: boolean) => {
    if (!settings) throw new Error("unavailable");
    if (settings.firstRunNoticeConfirmed !== true) {
      const { webdavSync: _, ...rest } = settings;
      await settingsApi.save({ ...rest, firstRunNoticeConfirmed: true });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }
    setAutomatic(false);
    if (connected) onConnected?.();
  };

  return automatic ? (
    <Suspense fallback={null}>
      <OnboardingWizard onFinish={handleFinish} />
    </Suspense>
  ) : null;
}
