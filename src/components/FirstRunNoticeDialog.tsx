import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { AppDialog } from "@/components/ui/app-dialog";
import { Button } from "@/components/ui/button";
import { useSettingsQuery } from "@/lib/query";
import { settingsApi } from "@/lib/api";

/** 首次运行欢迎提示：仅当后端启动阶段保留 firstRunNoticeConfirmed 为空时弹出。 */
export function FirstRunNoticeDialog() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();

  // 后端启动时已经决定好要不要弹：条件不满足的话字段会立即被写成 true，
  // 所以前端这里只需要判空即可——与其他既有确认标记的模式一致。
  const isOpen = settings != null && settings.firstRunNoticeConfirmed !== true;

  const handleAcknowledge = async () => {
    if (!settings) return;
    try {
      const { webdavSync: _, ...rest } = settings;
      await settingsApi.save({ ...rest, firstRunNoticeConfirmed: true });
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    } catch (error) {
      console.error("Failed to save firstRunNoticeConfirmed:", error);
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
