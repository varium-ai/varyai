import { useUpdate } from "@/contexts/UpdateContext";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Download, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";

interface UpdateBadgeProps {
  className?: string;
  onClick?: () => void;
}

export function UpdateBadge({ className = "", onClick }: UpdateBadgeProps) {
  const { hasUpdate, updateInfo, resetDismiss, isInstalling } = useUpdate();
  const { t } = useTranslation();
  const isActive = hasUpdate && updateInfo;
  const title = isActive
    ? t("settings.updateAvailable", {
        version: updateInfo?.availableVersion ?? "",
      })
    : t("settings.checkForUpdates");

  if (!isActive) {
    return null;
  }

  return (
    <Button
      type="button"
      variant="default"
      size="icon"
      title={title}
      aria-label={title}
      onClick={onClick ?? resetDismiss}
      disabled={isInstalling}
      className={cn("h-8 w-8 shrink-0 rounded-lg", className)}
    >
      {isInstalling ? (
        <Loader2 className="h-4 w-4 animate-spin" />
      ) : (
        <Download className="h-4 w-4" />
      )}
    </Button>
  );
}
