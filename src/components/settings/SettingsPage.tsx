import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { motion } from "framer-motion";
import {
  FolderSearch,
  Loader2,
  Save,
  Database,
  Cloud,
  ScrollText,
  HardDriveDownload,
  FlaskConical,
  type LucideIcon,
} from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import { settingsApi } from "@/lib/api";
import { LanguageSettings } from "@/components/settings/LanguageSettings";
import { PrivacySettings } from "@/components/settings/PrivacySettings";
import { ThemeSettings } from "@/components/settings/ThemeSettings";
import { NotificationPermissionSettings } from "@/components/settings/NotificationPermissionSettings";
import { WindowSettings } from "@/components/settings/WindowSettings";
import { AppVisibilitySettings } from "@/components/settings/AppVisibilitySettings";
import { DirectorySettings } from "@/components/settings/DirectorySettings";
import { ImportExportSection } from "@/components/settings/ImportExportSection";
import { BackupListSection } from "@/components/settings/BackupListSection";
import { WebdavSyncSection } from "@/components/settings/WebdavSyncSection";
import { AboutSection } from "@/components/settings/AboutSection";
import { LocalProxySettings } from "@/components/settings/LocalProxySettings";
import { LongConversationFallback } from "@/components/settings/LongConversationFallback";
import { GatewayKeysPanel } from "@/components/proxy/GatewayKeysPanel";
import { IntentRoutingPanel } from "@/components/routing/IntentRoutingPanel";
import { ConnectivityCheckConfigPanel } from "@/components/usage/ConnectivityCheckConfigPanel";
import { LogConfigPanel } from "@/components/settings/LogConfigPanel";
import { useSettings } from "@/hooks/useSettings";
import { useImportExport } from "@/hooks/useImportExport";
import { SettingsGroup } from "./SettingsGroup";
import "./settings-page.css";
import {
  useGlobalProxyConfig,
  useProxyStatusQuery,
  useProxyTakeoverStatus,
} from "@/lib/query/proxy";
import { DEFAULT_PROXY_PORT } from "@/types/proxy";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";

interface SettingsDialogProps {
  globalResourcesOnly?: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onImportSuccess?: () => void | Promise<void>;
  initialTab?: "general" | "routing" | "advanced" | "about";
}

export function SettingsPage({
  open,
  onOpenChange,
  onImportSuccess,
  initialTab = "general",
  globalResourcesOnly = false,
}: SettingsDialogProps) {
  const { t } = useTranslation();
  const {
    settings,
    isLoading,
    isSaving,
    isPortable,
    appConfigDir,
    resolvedDirs,
    updateSettings,
    updateDirectory,
    updateAppConfigDir,
    browseDirectory,
    browseAppConfigDir,
    resetDirectory,
    resetAppConfigDir,
    saveSettings,
    autoSaveSettings,
    requiresRestart,
    acknowledgeRestart,
  } = useSettings();

  const {
    selectedFile,
    status: importStatus,
    errorMessage,
    backupId,
    isImporting,
    selectImportFile,
    importConfig,
    exportConfig,
    clearSelection,
    resetStatus,
  } = useImportExport({ onImportSuccess });

  const [activeTab, setActiveTab] = useState<string>(initialTab);
  const [showRestartPrompt, setShowRestartPrompt] = useState(false);
  const tabScrollContainerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) {
      setActiveTab(initialTab);
      resetStatus();
    }
  }, [initialTab, open, resetStatus]);

  useEffect(() => {
    if (requiresRestart) {
      setShowRestartPrompt(true);
    }
  }, [requiresRestart]);

  useLayoutEffect(() => {
    if (tabScrollContainerRef.current) {
      tabScrollContainerRef.current.scrollTop = 0;
    }
  }, [activeTab]);

  const closeAfterSave = useCallback(() => {
    // 保存成功后关闭：不再重置语言，避免需要“保存两次”才生效
    acknowledgeRestart();
    clearSelection();
    resetStatus();
    onOpenChange(false);
  }, [acknowledgeRestart, clearSelection, onOpenChange, resetStatus]);

  const handleSave = useCallback(async () => {
    try {
      const result = await saveSettings(undefined, { silent: false });
      if (!result) return;
      if (result.requiresRestart) {
        setShowRestartPrompt(true);
        return;
      }
      closeAfterSave();
    } catch (error) {
      console.error("[SettingsPage] Failed to save settings", error);
    }
  }, [closeAfterSave, saveSettings]);

  const handleRestartLater = useCallback(() => {
    setShowRestartPrompt(false);
    closeAfterSave();
  }, [closeAfterSave]);

  const handleRestartNow = useCallback(async () => {
    setShowRestartPrompt(false);
    if (import.meta.env.DEV) {
      toast.success(t("settings.devModeRestartHint"), { closeButton: true });
      closeAfterSave();
      return;
    }

    try {
      await settingsApi.restart();
    } catch (error) {
      console.error("[SettingsPage] Failed to restart app", error);
      toast.error(t("settings.restartFailed"));
    } finally {
      closeAfterSave();
    }
  }, [closeAfterSave, t]);

  // 通用设置即时保存（无需手动点击）
  // 使用 autoSaveSettings 避免误触发系统 API（开机自启、Claude 插件等）
  // 返回保存是否成功：需要在保存成功后追加动作的调用方（如统一会话历史
  // 关闭后的备份还原）据此短路，其余调用方可忽略返回值。
  const handleAutoSave = useCallback(
    async (updates: Partial<SettingsFormState>): Promise<boolean> => {
      if (!settings) return false;
      // 乐观更新前捕获旧值：autoSaveSettings 发送的是全量表单状态，后端按
      // diff 触发副作用（如统一会话开关的 live 重写与历史迁移）。保存失败
      // 不回滚的话，失败的变更会滞留在表单里，被之后任意一次无关保存原样
      // 重放，绕过确认弹窗。
      const previousValues = Object.fromEntries(
        Object.keys(updates).map((key) => [
          key,
          settings[key as keyof SettingsFormState],
        ]),
      ) as Partial<SettingsFormState>;
      updateSettings(updates);
      try {
        await autoSaveSettings(updates);
        return true;
      } catch (error) {
        console.error("[SettingsPage] Failed to autosave settings", error);
        updateSettings(previousValues);
        toast.error(
          t("settings.saveFailedGeneric", {
            defaultValue: "保存失败，请重试",
          }),
        );
        return false;
      }
    },
    [autoSaveSettings, settings, t, updateSettings],
  );

  const isBusy = useMemo(() => isLoading && !settings, [isLoading, settings]);

  const tabItems = [
    { value: "general", label: t("settings.tabGeneral") },
    {
      value: "routing",
      label: t(globalResourcesOnly ? "gatewayKeys.title" : "settings.tabProxy"),
    },
    { value: "advanced", label: t("settings.tabAdvanced") },
    { value: "about", label: t("common.about") },
  ];

  return (
    <div className="settings-page mx-auto flex h-full w-full flex-col overflow-hidden">
      {isBusy ? (
        <div className="flex flex-1 items-center justify-center">
          <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
        </div>
      ) : (
        <Tabs
          value={activeTab}
          onValueChange={(tab) => {
            setActiveTab(tab);
          }}
          className="flex min-h-0 flex-1 flex-col"
        >
          <header className="settings-header">
            <h1 className="vary-page-title">{t("settings.title")}</h1>
            {activeTab === "general" && <p>{t("settings.statusNotice")}</p>}
            <div className="mt-4 flex flex-wrap">
              <TabsList
                aria-label={t("settings.categoryLabel")}
                slidingIndicator={false}
                className="h-auto max-w-full flex-wrap justify-start"
              >
                {tabItems.map((tab) => (
                  <TabsTrigger
                    key={tab.value}
                    value={tab.value}
                    className="min-h-9 min-w-0 px-3 py-1"
                  >
                    {tab.label}
                  </TabsTrigger>
                ))}
              </TabsList>
            </div>
          </header>

          <div className="flex-1 min-h-0 flex flex-col">
            <div
              ref={tabScrollContainerRef}
              className="flex-1 overflow-y-auto overflow-x-hidden pb-8"
            >
              <TabsContent value="general" className="mt-0">
                {settings ? (
                  <motion.div
                    initial={{ opacity: 0, y: 10 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration: 0.3 }}
                    className="space-y-[var(--workspace-module-gap)]"
                  >
                    <SettingsGroup title={t("settings.appearanceLanguage")}>
                      <LanguageSettings
                        value={settings.language}
                        onChange={(language) => handleAutoSave({ language })}
                      />
                      <ThemeSettings />
                    </SettingsGroup>
                    <SettingsGroup title={t("settings.appVisibility.title")}>
                      <AppVisibilitySettings
                        settings={settings}
                        onChange={handleAutoSave}
                        showAppChoices={!globalResourcesOnly}
                      />
                    </SettingsGroup>
                    <WindowSettings
                      settings={settings}
                      onChange={handleAutoSave}
                    />
                    <SettingsGroup title={t("privacy.title")}>
                      <div className="settings-detail">
                        <PrivacySettings />
                      </div>
                    </SettingsGroup>
                    <SettingsGroup title={t("settings.permissions.title")}>
                      <div className="settings-detail">
                        <NotificationPermissionSettings />
                        <p className="mt-3 text-xs text-muted-foreground">
                          {t("settings.permissions.deliveryHint")}
                        </p>
                      </div>
                    </SettingsGroup>
                    <LongConversationFallback />
                  </motion.div>
                ) : null}
              </TabsContent>

              <TabsContent value="routing" className="mt-0 pb-4">
                {settings ? (
                  <div className="space-y-[var(--workspace-module-gap)]">
                    {globalResourcesOnly ? (
                      <SharedGatewaySettings />
                    ) : (
                      <>
                        <LocalProxySettings
                          settings={settings}
                          onAutoSave={handleAutoSave}
                        />
                        <IntentRoutingPanel />
                      </>
                    )}
                  </div>
                ) : null}
              </TabsContent>

              <TabsContent value="advanced" className="space-y-6 mt-0 pb-4">
                {settings ? (
                  <motion.div
                    initial={{ opacity: 0, y: 10 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration: 0.3 }}
                    className="space-y-4"
                  >
                    <Accordion
                      type="multiple"
                      className="settings-list settings-advanced-list"
                    >
                      <AdvancedSettingsItem
                        value="directory"
                        icon={FolderSearch}
                        title={t("settings.advanced.configDir.title")}
                        description={t(
                          "settings.advanced.configDir.description",
                        )}
                      >
                        <DirectorySettings
                          appConfigDir={appConfigDir}
                          resolvedDirs={resolvedDirs}
                          onAppConfigChange={updateAppConfigDir}
                          onBrowseAppConfig={browseAppConfigDir}
                          onResetAppConfig={resetAppConfigDir}
                          claudeDir={settings.claudeConfigDir}
                          codexDir={settings.codexConfigDir}
                          geminiDir={settings.geminiConfigDir}
                          grokDir={settings.grokConfigDir}
                          opencodeDir={settings.opencodeConfigDir}
                          openclawDir={settings.openclawConfigDir}
                          hermesDir={settings.hermesConfigDir}
                          piDir={settings.piConfigDir}
                          workbuddyDir={settings.workbuddyConfigDir}
                          onDirectoryChange={updateDirectory}
                          onBrowseDirectory={browseDirectory}
                          onResetDirectory={resetDirectory}
                        />
                        <div className="settings-advanced-footer">
                          <p className="mr-auto text-xs text-muted-foreground">
                            {t("settings.advancedSaveScope")}
                          </p>
                          <Button
                            onClick={handleSave}
                            disabled={isSaving}
                            className="shrink-0"
                          >
                            {isSaving ? (
                              <span className="inline-flex items-center gap-2">
                                <Loader2 className="h-4 w-4 animate-spin" />
                                {t("settings.saving")}
                              </span>
                            ) : (
                              <>
                                <Save className="mr-2 h-4 w-4" />
                                {t("settings.saveDirectories")}
                              </>
                            )}
                          </Button>
                        </div>
                      </AdvancedSettingsItem>
                      <AdvancedSettingsItem
                        value="data"
                        icon={Database}
                        title={t("settings.advanced.data.title")}
                        description={t("settings.advanced.data.description")}
                      >
                        <ImportExportSection
                          status={importStatus}
                          selectedFile={selectedFile}
                          errorMessage={errorMessage}
                          backupId={backupId}
                          isImporting={isImporting}
                          onSelectFile={selectImportFile}
                          onImport={importConfig}
                          onExport={exportConfig}
                          onClear={clearSelection}
                        />
                      </AdvancedSettingsItem>
                      <AdvancedSettingsItem
                        value="backup"
                        icon={HardDriveDownload}
                        title={t("settings.advanced.backup.title", {
                          defaultValue: "Backup & Restore",
                        })}
                        description={t("settings.advanced.backup.description", {
                          defaultValue:
                            "Manage automatic backups, view and restore database snapshots",
                        })}
                      >
                        <BackupListSection
                          backupIntervalHours={settings.backupIntervalHours}
                          backupRetainCount={settings.backupRetainCount}
                          onSettingsChange={(updates) =>
                            handleAutoSave(updates)
                          }
                        />
                      </AdvancedSettingsItem>
                      <AdvancedSettingsItem
                        value="cloudSync"
                        icon={Cloud}
                        title={t("settings.advanced.cloudSync.title")}
                        description={t(
                          "settings.advanced.cloudSync.description",
                        )}
                      >
                        <WebdavSyncSection
                          config={settings?.webdavSync}
                          s3Config={settings?.s3Sync}
                          settings={settings}
                          onAutoSave={handleAutoSave}
                        />
                      </AdvancedSettingsItem>
                      <AdvancedSettingsItem
                        value="connectivityCheck"
                        icon={FlaskConical}
                        title={t("settings.advanced.connectivityCheck.title")}
                        description={t(
                          "settings.advanced.connectivityCheck.description",
                        )}
                      >
                        <ConnectivityCheckConfigPanel />
                      </AdvancedSettingsItem>
                      <AdvancedSettingsItem
                        value="logConfig"
                        icon={ScrollText}
                        title={t("settings.advanced.logConfig.title")}
                        description={t(
                          "settings.advanced.logConfig.description",
                        )}
                      >
                        <LogConfigPanel />
                      </AdvancedSettingsItem>
                    </Accordion>
                  </motion.div>
                ) : null}
              </TabsContent>

              <TabsContent value="about" className="mt-0">
                <AboutSection isPortable={isPortable} />
              </TabsContent>
            </div>
          </div>
        </Tabs>
      )}

      <Dialog
        open={showRestartPrompt}
        onOpenChange={(open) => !open && handleRestartLater()}
      >
        <DialogContent zIndex="alert" className="max-w-md glass border-border">
          <DialogHeader>
            <DialogTitle>{t("settings.restartRequired")}</DialogTitle>
          </DialogHeader>
          <div className="px-6">
            <p className="text-sm text-muted-foreground">
              {t("settings.restartRequiredMessage")}
            </p>
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={handleRestartLater}
              className="hover:bg-muted/50"
            >
              {t("settings.restartLater")}
            </Button>
            <Button
              onClick={handleRestartNow}
              className="bg-primary hover:bg-primary/90"
            >
              {t("settings.restartNow")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function AdvancedSettingsItem({
  value,
  icon: Icon,
  title,
  description,
  children,
}: {
  value: string;
  icon: LucideIcon;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <AccordionItem value={value} className="settings-advanced-item">
      <AccordionTrigger className="settings-advanced-trigger hover:no-underline">
        <span className="settings-advanced-icon" aria-hidden="true">
          <Icon size={16} strokeWidth={1.5} />
        </span>
        <span className="setting-copy">
          <span className="settings-advanced-title">{title}</span>
          <span className="settings-advanced-description">{description}</span>
        </span>
      </AccordionTrigger>
      <AccordionContent className="settings-advanced-content py-4">
        {children}
      </AccordionContent>
    </AccordionItem>
  );
}

function SharedGatewaySettings() {
  const { data, isError } = useProxyStatusQuery();
  const { data: takeoverStatus } = useProxyTakeoverStatus();
  const { data: gatewayConfig } = useGlobalProxyConfig();
  const port = data?.port || gatewayConfig?.listenPort || DEFAULT_PROXY_PORT;
  return (
    <GatewayKeysPanel
      port={String(port)}
      running={data?.running}
      statusUnavailable={isError}
      takeoverStatus={takeoverStatus}
    />
  );
}
