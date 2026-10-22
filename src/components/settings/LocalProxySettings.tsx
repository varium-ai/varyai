import { useState } from "react";
import { Activity, Server } from "lucide-react";
import { motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { Badge } from "@/components/ui/badge";
import { ProxyPanel } from "@/components/proxy";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import type { SettingsFormState } from "@/hooks/useSettings";
import { proxyApi } from "@/lib/api/proxy";
import { ROUTING_APP_IDS } from "@/config/appConfig";

interface LocalProxySettingsProps {
  settings: SettingsFormState;
  onAutoSave: (updates: Partial<SettingsFormState>) => Promise<boolean | void>;
}

export function LocalProxySettings({
  settings,
  onAutoSave,
}: LocalProxySettingsProps) {
  const { t } = useTranslation();
  const [showProxyConfirm, setShowProxyConfirm] = useState(false);
  const [isTogglingAllApps, setIsTogglingAllApps] = useState(false);

  const {
    isRunning,
    startProxyServer,
    stopWithRestore,
    setTakeoverForApp,
    isPending: isProxyPending,
  } = useProxyStatus();

  const setProxyEnabled = async (enabled: boolean) => {
    setIsTogglingAllApps(true);
    try {
      if (enabled) await startProxyServer();

      for (const appType of ROUTING_APP_IDS) {
        if (appType === "doubaowork" && enabled) {
          if (!settings.visibleApps?.doubaowork) continue;
          if (!(await proxyApi.getAgentInstallations()).doubaowork) continue;
        }
        if (
          appType === "doubaowork" &&
          !enabled &&
          !settings.visibleApps?.doubaowork
        )
          continue;
        try {
          await setTakeoverForApp({ appType, enabled });
        } catch (error) {
          console.error(`Toggle ${appType} takeover failed:`, error);
        }
      }

      if (!enabled) await stopWithRestore();
    } finally {
      setIsTogglingAllApps(false);
    }
  };

  const handleToggleProxy = async (checked: boolean) => {
    try {
      if (checked && !settings?.proxyConfirmed) {
        setShowProxyConfirm(true);
      } else {
        await setProxyEnabled(checked);
      }
    } catch (error) {
      console.error("Toggle proxy failed:", error);
    }
  };

  const handleProxyConfirm = async () => {
    setShowProxyConfirm(false);
    try {
      await onAutoSave({ proxyConfirmed: true });
      await setProxyEnabled(true);
    } catch (error) {
      console.error("Proxy confirm failed:", error);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className="space-y-4"
    >
      <Accordion type="multiple" defaultValue={[]} className="w-full space-y-4">
        <AccordionItem
          value="localProxy"
          className="rounded-xl glass-card overflow-hidden"
        >
          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
            <div className="flex items-center gap-3">
              <Server className="h-5 w-5 text-green-500" />
              <div className="text-left">
                <h3 className="text-base font-semibold">
                  {t("settings.advanced.proxy.title")}
                </h3>
                <p className="text-sm text-muted-foreground font-normal">
                  {t("settings.advanced.proxy.description")}
                </p>
              </div>
              <Badge
                variant={isRunning ? "default" : "secondary"}
                className="gap-1.5 h-6 ml-auto mr-2"
              >
                <Activity
                  className={`h-3 w-3 ${isRunning ? "status-heartbeat" : ""}`}
                />
                {isRunning
                  ? t("settings.advanced.proxy.running")
                  : t("settings.advanced.proxy.stopped")}
              </Badge>
            </div>
          </AccordionTrigger>
          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
            <ProxyPanel
              onToggleProxy={handleToggleProxy}
              isProxyPending={isProxyPending || isTogglingAllApps}
            />
          </AccordionContent>
        </AccordionItem>
      </Accordion>

      <ConfirmDialog
        isOpen={showProxyConfirm}
        variant="info"
        title={t("confirm.proxy.title")}
        message={t("confirm.proxy.message")}
        confirmText={t("confirm.proxy.confirm")}
        onConfirm={() => void handleProxyConfirm()}
        onCancel={() => setShowProxyConfirm(false)}
      />
    </motion.div>
  );
}
