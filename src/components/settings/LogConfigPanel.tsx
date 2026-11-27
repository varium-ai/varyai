import { useEffect, useId, useRef, useState } from "react";
import { Download, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { settingsApi, type LogConfig } from "@/lib/api/settings";

const LOG_LEVELS = ["error", "warn", "info", "debug", "trace"] as const;

export function LogConfigPanel() {
  const { t } = useTranslation();
  const enabledId = useId();
  const levelId = useId();
  const [config, setConfig] = useState<LogConfig>({
    enabled: true,
    level: "info",
  });
  const [isLoading, setIsLoading] = useState(true);
  const [isExporting, setIsExporting] = useState(false);
  const exportBusy = useRef(false);

  const exportLogs = async () => {
    if (exportBusy.current) return;
    exportBusy.current = true;
    setIsExporting(true);
    try {
      const result = await settingsApi.exportApplicationLog();
      if (result) {
        toast.success(t("settings.advanced.logConfig.exportSuccess"));
      }
    } catch (error) {
      const reasons: Record<string, string> = {
        "diagnostics.logMissing": "exportMissing",
        "diagnostics.logEmpty": "exportEmpty",
        "diagnostics.sameSource": "exportSameSource",
        "diagnostics.backendUpdateRequired": "exportBackendUpdate",
      };
      const key =
        typeof error === "string" && Object.hasOwn(reasons, error)
          ? reasons[error]
          : undefined;
      toast.error(t(`settings.advanced.logConfig.${key ?? "exportFailed"}`));
    } finally {
      exportBusy.current = false;
      setIsExporting(false);
    }
  };

  useEffect(() => {
    settingsApi
      .getLogConfig()
      .then(setConfig)
      .catch((e) => console.error("Failed to load log config:", e))
      .finally(() => setIsLoading(false));
  }, []);

  const handleChange = async (updates: Partial<LogConfig>) => {
    const newConfig = { ...config, ...updates };
    setConfig(newConfig);
    try {
      await settingsApi.setLogConfig(newConfig);
    } catch (e) {
      console.error("Failed to save log config:", e);
      toast.error(String(e));
      setConfig(config);
    }
  };

  const exportLabel = t(
    isExporting
      ? "settings.advanced.logConfig.exporting"
      : "settings.advanced.logConfig.export",
  );

  return (
    <div className="settings-advanced-section">
      <div className="settings-advanced-actions justify-between">
        <p className="setting-copy settings-advanced-help">
          {t("settings.advanced.logConfig.exportDescription")}
        </p>
        <Button
          variant="outline"
          className="settings-log-export ml-auto shrink-0"
          aria-label={exportLabel}
          aria-busy={isExporting}
          title={exportLabel}
          disabled={isExporting}
          onClick={exportLogs}
        >
          {isExporting ? (
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          ) : (
            <Download className="h-4 w-4" aria-hidden="true" />
          )}
          <span>{exportLabel}</span>
        </Button>
      </div>
      <div className="settings-list settings-log-options">
        <div className="setting-row">
          <div className="setting-copy space-y-1">
            <Label htmlFor={enabledId}>
              {t("settings.advanced.logConfig.enabled")}
            </Label>
            <p className="settings-advanced-help">
              {t("settings.advanced.logConfig.enabledDescription")}
            </p>
          </div>
          <Switch
            id={enabledId}
            checked={config.enabled}
            disabled={isLoading}
            onCheckedChange={(checked) => handleChange({ enabled: checked })}
          />
        </div>
        <div className="setting-row">
          <div className="setting-copy space-y-1">
            <Label htmlFor={levelId}>
              {t("settings.advanced.logConfig.level")}
            </Label>
            <p className="settings-advanced-help">
              {t("settings.advanced.logConfig.levelDescription")}
            </p>
          </div>
          <Select
            value={config.level}
            disabled={isLoading || !config.enabled}
            onValueChange={(value) =>
              handleChange({ level: value as LogConfig["level"] })
            }
          >
            <SelectTrigger id={levelId} className="w-[120px] max-w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {LOG_LEVELS.map((level) => (
                <SelectItem key={level} value={level}>
                  {t(`settings.advanced.logConfig.levels.${level}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {/* Diagnostic log levels. */}
      <div className="settings-list settings-log-guide">
        <p className="settings-advanced-subtitle">
          {t("settings.advanced.logConfig.levelHint")}
        </p>
        <div className="settings-advanced-help grid gap-1">
          <p>
            <span className="font-mono text-destructive">error</span> -{" "}
            {t("settings.advanced.logConfig.levelDesc.error")}
          </p>
          <p>
            <span className="font-mono text-[var(--warning)]">warn</span> -{" "}
            {t("settings.advanced.logConfig.levelDesc.warn")}
          </p>
          <p>
            <span className="font-mono text-[hsl(var(--link-foreground))]">
              info
            </span>{" "}
            - {t("settings.advanced.logConfig.levelDesc.info")}
          </p>
          <p>
            <span className="font-mono text-[var(--success)]">debug</span> -{" "}
            {t("settings.advanced.logConfig.levelDesc.debug")}
          </p>
          <p>
            <span className="font-mono text-muted-foreground">trace</span> -{" "}
            {t("settings.advanced.logConfig.levelDesc.trace")}
          </p>
        </div>
      </div>
    </div>
  );
}
