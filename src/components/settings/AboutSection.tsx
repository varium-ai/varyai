import { useEffect, useState } from "react";
import { Globe, Info, Loader2, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { getVersion } from "@tauri-apps/api/app";
import { settingsApi } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { motion } from "framer-motion";
import appIcon from "@/assets/icons/app-icon.png";
import { APP_DISPLAY_NAME, OFFICIAL_WEBSITE_URL } from "@/config/brand";
import { useUpdate } from "@/contexts/UpdateContext";

interface AboutSectionProps {
  isPortable: boolean;
}

// Keep the local app version visible when the About tab remounts.
let appVersionCache: string | null = null;

export function AboutSection({ isPortable }: AboutSectionProps) {
  const { t } = useTranslation();
  const {
    checkUpdate,
    resetDismiss,
    isChecking: isCheckingAppUpdate,
  } = useUpdate();
  const [version, setVersion] = useState<string | null>(() => appVersionCache);
  const [isLoadingVersion, setIsLoadingVersion] = useState(
    () => appVersionCache === null,
  );

  const handleCheckAppUpdate = async () => {
    resetDismiss();
    try {
      const available = await checkUpdate();
      if (!available) toast.success(t("settings.upToDate"));
    } catch {
      toast.error(t("settings.checkUpdateFailed"));
    }
  };

  useEffect(() => {
    let active = true;
    const loadAppVersion = async () => {
      try {
        const appVersion = await getVersion();
        appVersionCache = appVersion;
        if (active) setVersion(appVersion);
      } catch {
        if (active) setVersion(null);
      } finally {
        if (active) setIsLoadingVersion(false);
      }
    };
    void loadAppVersion();
    return () => {
      active = false;
    };
  }, []);

  const displayVersion = version ?? t("common.unknown");

  return (
    <motion.section
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className="space-y-6"
    >
      <motion.div
        initial={{ opacity: 0, scale: 0.98 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.3, delay: 0.1 }}
        className="rounded-xl border border-border bg-gradient-to-br from-card/80 to-card/40 p-6 space-y-5 shadow-sm"
      >
        <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center gap-8">
            <div className="flex flex-col items-center gap-2">
              <div className="flex items-center gap-2">
                <img src={appIcon} alt={APP_DISPLAY_NAME} className="h-5 w-5" />
                <h4 className="text-lg font-semibold text-foreground">
                  {APP_DISPLAY_NAME}
                </h4>
              </div>
              <div className="flex items-center gap-2">
                <Badge variant="outline" className="gap-1.5 bg-background/80">
                  <span className="text-muted-foreground">
                    {t("common.version")}
                  </span>
                  {isLoadingVersion ? (
                    <Loader2 className="h-3 w-3 animate-spin" />
                  ) : (
                    <span className="font-medium">{`v${displayVersion}`}</span>
                  )}
                </Badge>
                {isPortable && (
                  <Badge variant="secondary" className="gap-1.5">
                    <Info className="h-3 w-3" />
                    {t("settings.portableMode")}
                  </Badge>
                )}
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void handleCheckAppUpdate()}
              disabled={isCheckingAppUpdate}
              className="h-8 gap-1.5 text-xs"
            >
              {isCheckingAppUpdate ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <RefreshCw className="h-3.5 w-3.5" />
              )}
              {isCheckingAppUpdate
                ? t("settings.checking")
                : t("settings.checkForUpdates")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => settingsApi.openExternal(OFFICIAL_WEBSITE_URL)}
              className="h-8 gap-1.5 text-xs"
            >
              <Globe className="h-3.5 w-3.5" />
              {t("settings.officialWebsite")}
            </Button>
          </div>
        </div>
      </motion.div>
    </motion.section>
  );
}
