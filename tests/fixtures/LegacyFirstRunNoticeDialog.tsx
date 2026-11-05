import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { AppDialog } from "@/components/ui/app-dialog";
import { Button } from "@/components/ui/button";
import { useSettingsQuery } from "@/lib/query";
import { settingsApi } from "@/lib/api";

/** Historical welcome notice used by the legacy shell compatibility fixture. */
export function FirstRunNoticeDialog() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();

  const isOpen = settings != null && settings.firstRunNoticeConfirmed !== true;

  const handleAcknowledge = async () => {
    if (!settings) return;
    try {
      const { webdavSync: _, ...rest } = settings;
      await settingsApi.save({ ...rest, firstRunNoticeConfirmed: true });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    } catch {
      console.error("Failed to acknowledge the legacy welcome notice.");
    }
  };

  return (
    <AppDialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open) void handleAcknowledge();
      }}
      zIndex="top"
      title={t("firstRunNotice.title")}
      description={
        <>
          <span className="block">{t("firstRunNotice.bodyDefault")}</span>
          <span className="mt-3 block">{t("firstRunNotice.bodyOfficial")}</span>
        </>
      }
      footer={
        <Button onClick={handleAcknowledge}>
          {t("firstRunNotice.confirm")}
        </Button>
      }
    />
  );
}
