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
    enabled: settings != null,
  });
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!settings || !records.isSuccess) return;
    if (records.data.length < 1) {
      setOpen(true);
    }
  }, [settings, records.isSuccess, records.data]);

  const handleFinish = async (connected: boolean) => {
    if (!settings) throw new Error("unavailable");
    if (!connected && (!records.data || records.data.length < 1)) {
      return;
    }
    if (settings.firstRunNoticeConfirmed !== true) {
      const { webdavSync: _, ...rest } = settings;
      await settingsApi.save({ ...rest, firstRunNoticeConfirmed: true });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }
    setOpen(false);
    if (connected) onConnected?.();
  };

  return open ? (
    <Suspense fallback={null}>
      <OnboardingWizard onFinish={handleFinish} />
    </Suspense>
  ) : null;
}
