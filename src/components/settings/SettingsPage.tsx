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
  CheckCircle2,
  FolderSearch,
  AppWindow,
  Bell,
  Loader2,
  RefreshCw,
  Save,
  Server,
  SlidersHorizontal,
  type LucideIcon,
  Database,
  Cloud,
  ScrollText,
  HardDriveDownload,
  FlaskConical,
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
import { TerminalSettings } from "@/components/settings/TerminalSettings";
import { DirectorySettings } from "@/components/settings/DirectorySettings";
import { ImportExportSection } from "@/components/settings/ImportExportSection";
import { BackupListSection } from "@/components/settings/BackupListSection";
import { WebdavSyncSection } from "@/components/settings/WebdavSyncSection";
import { AboutSection } from "@/components/settings/AboutSection";
import { LocalProxySettings } from "@/components/settings/LocalProxySettings";
import { ConnectivityCheckConfigPanel } from "@/components/usage/ConnectivityCheckConfigPanel";
import { LogConfigPanel } from "@/components/settings/LogConfigPanel";
import { useSettings } from "@/hooks/useSettings";
import { useImportExport } from "@/hooks/useImportExport";
import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";

interface SettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onImportSuccess?: () => void | Promise<void>;
  initialTab?: "general" | "advanced" | "about";
}

interface SettingsPanelProps {
  icon: LucideIcon;
  title: string;
  description?: string;
  children: ReactNode;
  className?: string;
  flush?: boolean;
  hideHeader?: boolean;
}

function SettingsPanel({
  icon: Icon,
  title,
  description,
  children,
  className = "",
  flush = false,
  hideHeader = false,
}: SettingsPanelProps) {
  return (
    <section
      className={cn(
        "rounded-2xl border border-slate-200/90 bg-white p-5 shadow-sm dark:border-border dark:bg-card sm:p-6",
        flush && "p-0",
        className,
      )}
    >
      {!hideHeader && (
        <div
          className={cn(
            "mb-5 flex items-start gap-3.5",
            flush && "px-5 pt-5 sm:px-6 sm:pt-6",
          )}
        >
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border border-blue-100 bg-blue-50 text-blue-600 shadow-sm dark:border-blue-500/20 dark:bg-blue-950/30 dark:text-blue-300">
            <Icon className="h-5 w-5" />
          </div>
          <div className="min-w-0">
            <h2 className="text-base font-bold tracking-tight text-slate-900 dark:text-foreground">
              {title}
            </h2>
            {description ? (
              <p className="mt-1 text-xs leading-relaxed text-slate-500 dark:text-muted-foreground">
                {description}
              </p>
            ) : null}
          </div>
        </div>
      )}
      {children}
    </section>
  );
}

export function SettingsPage({
  open,
  onOpenChange,
  onImportSuccess,
  initialTab = "general",
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
    { value: "advanced", label: t("settings.tabAdvanced") },
    { value: "about", label: t("common.about") },
  ];

  const statusLabel = isSaving
    ? t("settings.statusSaving")
    : requiresRestart
      ? t("settings.statusRestartRequired")
      : t("settings.statusReady");

  return (
    <div className="mx-auto flex h-full w-full max-w-6xl flex-col overflow-hidden px-5 pt-5 sm:px-8">
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
          <div className="space-y-5 pb-5">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <span className="shrink-0 text-sm font-medium text-slate-500 dark:text-muted-foreground">
                {t("settings.categoryLabel")}
              </span>
              <TabsList
                slidingIndicator={false}
                className="-m-1 flex h-auto max-w-full flex-wrap justify-start gap-2 overflow-visible bg-transparent p-1"
              >
                {tabItems.map((tab) => (
                  <TabsTrigger
                    key={tab.value}
                    value={tab.value}
                    className="min-w-0 rounded-xl border border-slate-200 bg-white px-4 py-1.5 text-sm font-medium text-slate-700 shadow-none transition-all data-[state=active]:border-blue-600 data-[state=active]:bg-white data-[state=active]:text-blue-600 data-[state=active]:shadow-sm data-[state=inactive]:opacity-100 data-[state=inactive]:hover:border-slate-300 data-[state=inactive]:hover:bg-slate-50 dark:border-border dark:bg-card dark:text-slate-300 dark:data-[state=active]:border-blue-400 dark:data-[state=active]:bg-card dark:data-[state=active]:text-blue-300 dark:data-[state=inactive]:hover:bg-muted"
                  >
                    {tab.label}
                  </TabsTrigger>
                ))}
              </TabsList>
            </div>

            <div className="flex items-center justify-between gap-4 rounded-2xl border border-slate-200/90 bg-white px-5 py-3.5 shadow-sm dark:border-border dark:bg-card">
              <div className="flex min-w-0 items-center gap-2.5">
                <span className="relative flex h-2 w-2 shrink-0">
                  <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75" />
                  <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-500" />
                </span>
                <p className="truncate text-sm text-slate-700 dark:text-slate-200">
                  {t("settings.statusNotice")}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-1.5 text-xs font-semibold text-emerald-600 dark:text-emerald-400">
                {isSaving ? (
                  <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <CheckCircle2 className="h-3.5 w-3.5" />
                )}
                <span>{statusLabel}</span>
              </div>
            </div>
          </div>

          <div className="flex-1 min-h-0 flex flex-col">
            <div
              ref={tabScrollContainerRef}
              className="flex-1 overflow-y-auto overflow-x-hidden pr-2 pb-8"
            >
              <TabsContent value="general" className="mt-0">
                {settings ? (
                  <motion.div
                    initial={{ opacity: 0, y: 10 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration: 0.3 }}
                    className="space-y-5"
                  >
                    <SettingsPanel
                      icon={SlidersHorizontal}
                      title={t("settings.general")}
                      description={t("settings.statusNotice")}
                      hideHeader
                    >
                      <div className="grid gap-6 md:grid-cols-2">
                        <LanguageSettings
                          value={settings.language}
                          onChange={(lang) =>
                            handleAutoSave({ language: lang })
                          }
                        />
                        <ThemeSettings />
                      </div>
                    </SettingsPanel>

                    <SettingsPanel
                      icon={SlidersHorizontal}
                      title={t("privacy.title")}
                    >
                      <PrivacySettings />
                    </SettingsPanel>

                    <SettingsPanel
                      icon={Server}
                      title={t("settings.appVisibility.title")}
                      description={t("settings.appVisibility.description")}
                    >
                      <div className="space-y-6">
                        <AppVisibilitySettings
                          settings={settings}
                          onChange={handleAutoSave}
                        />
                      </div>
                    </SettingsPanel>

                    <SettingsPanel
                      icon={Bell}
                      title={t("settings.permissions.title")}
                      description={t("settings.permissions.deliveryHint")}
                    >
                      <NotificationPermissionSettings />
                    </SettingsPanel>

                    <SettingsPanel
                      icon={AppWindow}
                      title={t("settings.windowBehavior")}
                    >
                      <div className="space-y-6">
                        <div className="grid gap-6 border-t border-slate-200/80 pt-6 dark:border-border md:grid-cols-2">
                          <WindowSettings
                            settings={settings}
                            onChange={handleAutoSave}
                          />
                          <TerminalSettings
                            value={settings.preferredTerminal}
                            onChange={(terminal) =>
                              handleAutoSave({ preferredTerminal: terminal })
                            }
                          />
                        </div>
                      </div>
                    </SettingsPanel>
                  </motion.div>
                ) : null}
              </TabsContent>

              <TabsContent value="advanced" className="space-y-6 mt-0 pb-4">
                {settings ? (
                  <SettingsPanel
                    icon={SlidersHorizontal}
                    title={t("settings.tabAdvanced")}
                    description={t("settings.advanced.configDir.description")}
                    flush
                    hideHeader
                  >
                    <motion.div
                      initial={{ opacity: 0, y: 10 }}
                      animate={{ opacity: 1, y: 0 }}
                      transition={{ duration: 0.3 }}
                      className="space-y-4 p-5 sm:p-6"
                    >
                      <LocalProxySettings
                        settings={settings}
                        onAutoSave={handleAutoSave}
                      />

                      <Accordion
                        type="multiple"
                        defaultValue={[]}
                        className="w-full space-y-4"
                      >
                        <AccordionItem
                          value="directory"
                          className="rounded-xl glass-card overflow-hidden"
                        >
                          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
                            <div className="flex items-center gap-3">
                              <FolderSearch className="h-5 w-5 text-primary" />
                              <div className="text-left">
                                <h3 className="text-base font-semibold">
                                  {t("settings.advanced.configDir.title")}
                                </h3>
                                <p className="text-sm font-normal text-muted-foreground">
                                  {t("settings.advanced.configDir.description")}
                                </p>
                              </div>
                            </div>
                          </AccordionTrigger>
                          <AccordionContent className="border-t border-border/50 px-6 pb-6 pt-4">
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
                          </AccordionContent>
                        </AccordionItem>

                        <AccordionItem
                          value="data"
                          className="rounded-xl glass-card overflow-hidden"
                        >
                          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
                            <div className="flex items-center gap-3">
                              <Database className="h-5 w-5 text-blue-500" />
                              <div className="text-left">
                                <h3 className="text-base font-semibold">
                                  {t("settings.advanced.data.title")}
                                </h3>
                                <p className="text-sm text-muted-foreground font-normal">
                                  {t("settings.advanced.data.description")}
                                </p>
                              </div>
                            </div>
                          </AccordionTrigger>
                          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
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
                          </AccordionContent>
                        </AccordionItem>

                        <AccordionItem
                          value="backup"
                          className="rounded-xl glass-card overflow-hidden"
                        >
                          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
                            <div className="flex items-center gap-3">
                              <HardDriveDownload className="h-5 w-5 text-amber-500" />
                              <div className="text-left">
                                <h3 className="text-base font-semibold">
                                  {t("settings.advanced.backup.title", {
                                    defaultValue: "Backup & Restore",
                                  })}
                                </h3>
                                <p className="text-sm text-muted-foreground font-normal">
                                  {t("settings.advanced.backup.description", {
                                    defaultValue:
                                      "Manage automatic backups, view and restore database snapshots",
                                  })}
                                </p>
                              </div>
                            </div>
                          </AccordionTrigger>
                          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
                            <BackupListSection
                              backupIntervalHours={settings.backupIntervalHours}
                              backupRetainCount={settings.backupRetainCount}
                              onSettingsChange={(updates) =>
                                handleAutoSave(updates)
                              }
                            />
                          </AccordionContent>
                        </AccordionItem>

                        <AccordionItem
                          value="cloudSync"
                          className="rounded-xl glass-card overflow-hidden"
                        >
                          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
                            <div className="flex items-center gap-3">
                              <Cloud className="h-5 w-5 text-blue-500" />
                              <div className="text-left">
                                <h3 className="text-base font-semibold">
                                  {t("settings.advanced.cloudSync.title")}
                                </h3>
                                <p className="text-sm text-muted-foreground font-normal">
                                  {t("settings.advanced.cloudSync.description")}
                                </p>
                              </div>
                            </div>
                          </AccordionTrigger>
                          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
                            <WebdavSyncSection
                              config={settings?.webdavSync}
                              s3Config={settings?.s3Sync}
                              settings={settings}
                              onAutoSave={handleAutoSave}
                            />
                          </AccordionContent>
                        </AccordionItem>

                        <AccordionItem
                          value="connectivityCheck"
                          className="rounded-xl glass-card overflow-hidden"
                        >
                          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
                            <div className="flex items-center gap-3">
                              <FlaskConical className="h-5 w-5 text-emerald-500" />
                              <div className="text-left">
                                <h3 className="text-base font-semibold">
                                  {t(
                                    "settings.advanced.connectivityCheck.title",
                                  )}
                                </h3>
                                <p className="text-sm text-muted-foreground font-normal">
                                  {t(
                                    "settings.advanced.connectivityCheck.description",
                                  )}
                                </p>
                              </div>
                            </div>
                          </AccordionTrigger>
                          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
                            <ConnectivityCheckConfigPanel />
                          </AccordionContent>
                        </AccordionItem>

                        <AccordionItem
                          value="logConfig"
                          className="rounded-xl glass-card overflow-hidden"
                        >
                          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
                            <div className="flex items-center gap-3">
                              <ScrollText className="h-5 w-5 text-cyan-500" />
                              <div className="text-left">
                                <h3 className="text-base font-semibold">
                                  {t("settings.advanced.logConfig.title")}
                                </h3>
                                <p className="text-sm text-muted-foreground font-normal">
                                  {t("settings.advanced.logConfig.description")}
                                </p>
                              </div>
                            </div>
                          </AccordionTrigger>
                          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
                            <LogConfigPanel />
                          </AccordionContent>
                        </AccordionItem>
                      </Accordion>
                    </motion.div>
                  </SettingsPanel>
                ) : null}
              </TabsContent>

              <TabsContent value="about" className="mt-0">
                <AboutSection isPortable={isPortable} />
              </TabsContent>
            </div>

            {activeTab === "advanced" && settings && (
              <div
                className="-mx-5 flex-shrink-0 border-t border-border-default pt-4 pb-4 sm:-mx-8"
                style={{ backgroundColor: "hsl(var(--background))" }}
              >
                <div className="flex items-center justify-end gap-3 px-5 sm:px-8">
                  <Button onClick={handleSave} disabled={isSaving}>
                    {isSaving ? (
                      <span className="inline-flex items-center gap-2">
                        <Loader2 className="h-4 w-4 animate-spin" />
                        {t("settings.saving")}
                      </span>
                    ) : (
                      <>
                        <Save className="mr-2 h-4 w-4" />
                        {t("common.save")}
                      </>
                    )}
                  </Button>
                </div>
              </div>
            )}
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
