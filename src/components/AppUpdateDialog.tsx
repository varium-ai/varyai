import { useEffect, useState } from "react";
import { Download, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useUpdate } from "@/contexts/UpdateContext";
import { settingsApi } from "@/lib/api";
import { RELEASES_LATEST_URL } from "@/config/brand";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

export function AppUpdateDialog() {
  const { t } = useTranslation();
  const {
    hasUpdate,
    updateInfo,
    isDismissed,
    isInstalling,
    error,
    dismissUpdate,
    installUpdate,
  } = useUpdate();
  const [isPortable, setIsPortable] = useState<boolean | null>(null);

  useEffect(() => {
    let active = true;
    void settingsApi
      .isPortable()
      .then((portable) => {
        if (active) setIsPortable(portable);
      })
      .catch(() => {
        if (active) setIsPortable(false);
      });

    return () => {
      active = false;
    };
  }, []);

  const isOpen = hasUpdate && Boolean(updateInfo) && !isDismissed;
  if (!isOpen || isPortable === null || !updateInfo) return null;

  const handleInstall = async () => {
    try {
      await installUpdate();
    } catch {
      toast.error(t("settings.updateInstallFailed"));
    }
  };

  const handleManualDownload = async () => {
    try {
      await settingsApi.openExternal(RELEASES_LATEST_URL);
    } catch {
      toast.error(t("settings.openReleaseNotesFailed"));
    }
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open && !isInstalling) dismissUpdate();
      }}
    >
      <DialogContent
        className="max-w-md overflow-hidden"
        zIndex="top"
        onOpenAutoFocus={(event) => event.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>
            {t("settings.updateAvailable", {
              version: updateInfo.availableVersion,
            })}
          </DialogTitle>
          <DialogDescription>
            {t("settings.updateDialogDescription", {
              version: updateInfo.availableVersion,
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3 px-6 py-5">
          {updateInfo.notes && (
            <div className="max-h-48 overflow-y-auto rounded-md bg-muted/40 p-3 text-sm text-foreground/90">
              <p className="mb-2 font-medium">{t("settings.releaseNotes")}</p>
              <p className="whitespace-pre-wrap">{updateInfo.notes}</p>
            </div>
          )}
          {error && (
            <p role="alert" className="text-sm text-destructive">
              {t("settings.updateInstallFailed")}
            </p>
          )}
          {!isPortable && (
            <Button
              type="button"
              variant="link"
              size="sm"
              className="h-auto p-0"
              onClick={() => void handleManualDownload()}
            >
              {t("settings.viewReleaseNotes")}
            </Button>
          )}
          {isPortable && (
            <p className="text-sm text-muted-foreground">
              {t("settings.portableMode")}
            </p>
          )}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            disabled={isInstalling}
            onClick={dismissUpdate}
          >
            {t("settings.updateLater")}
          </Button>
          {isPortable ? (
            <Button type="button" onClick={() => void handleManualDownload()}>
              <Download className="mr-2 h-4 w-4" />
              {t("settings.downloadUpdate")}
            </Button>
          ) : (
            <Button
              type="button"
              disabled={isInstalling}
              onClick={() => void handleInstall()}
            >
              {isInstalling ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <Download className="mr-2 h-4 w-4" />
              )}
              {isInstalling
                ? t("settings.updating")
                : t("settings.updateTo", {
                    version: updateInfo.availableVersion,
                  })}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
