import { useId, useMemo, useState, type ReactNode } from "react";
import {
  AlertTriangle,
  ChevronRight,
  Copy,
  Edit2,
  LockKeyhole,
  Plus,
  Trash2,
} from "lucide-react";
import { AnimatePresence, motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import { settingsApi, type AppId } from "@/lib/api";
import type { Provider, VisibleApps } from "@/types";
import { isTakeoverAppId } from "@/config/appConfig";
import { useSecurityEventsSummary } from "@/hooks/useSecurityEvents";
import { useSecurityProtection } from "@/hooks/useSecurityProtection";
import { resolveUsageRange } from "@/lib/usageRange";
import { ProviderIcon } from "@/components/ProviderIcon";
import { resolveProviderIcon } from "@/utils/providerIcon";
import { resolveCodexOfficialIdentity } from "@/utils/providerCapabilities";
import { cn } from "@/lib/utils";
import { QoderCertificatePanel } from "@/components/QoderCertificatePanel";
import { DoubaoWorkPanel } from "@/components/DoubaoWorkPanel";
import { ProviderList } from "@/components/providers/ProviderList";
import { ManagedAppTabs } from "@/components/home/ManagedAppTabs";

const LOCAL_PROXY_PORT = 15721;

interface HomeDashboardProps {
  activeApp: AppId;
  projectSwitcher?: ReactNode;
  visibleApps: VisibleApps;
  providers: Record<string, Provider>;
  currentProviderId: string;
  activeProviderId?: string;
  isLoading?: boolean;
  onAppChange: (app: AppId) => void;
  onCreate: () => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
  onDuplicate: (provider: Provider) => void;
  onSwitch: (provider: Provider) => void;
  onOpenSecurity: () => void;
  onOpenRoutingSettings: () => void;
  onManageAgents?: () => void;
}

function getFirstString(values: unknown[]): string | undefined {
  return values
    .find(
      (value): value is string =>
        typeof value === "string" && value.trim().length > 0,
    )
    ?.trim();
}

function getProviderModel(
  provider: Provider,
  appId: AppId,
): string | undefined {
  const config = provider.settingsConfig ?? {};
  const models = Array.isArray(config.models) ? config.models : [];
  const firstModel = models[0];

  if (
    ["qoder", "trae", "kimi", "qwen", "zcode"].includes(appId) &&
    models.length > 0
  )
    return models
      .map((model: any) => model.model)
      .filter(Boolean)
      .join(" · ");
  if (appId === "workbuddy" && models.length > 0) {
    return models
      .map((model) => model.id)
      .filter(Boolean)
      .join(", ");
  }

  return getFirstString([
    config.model,
    config.modelName,
    appId === "workbuddy" ? config.id : undefined,
    config.env?.ANTHROPIC_MODEL,
    config.env?.GEMINI_MODEL,
    typeof firstModel === "string" ? firstModel : firstModel?.id,
  ]);
}

function getProviderEndpoint(
  provider: Provider,
  appId: AppId,
): string | undefined {
  const config = provider.settingsConfig ?? {};
  return getFirstString([
    provider.websiteUrl,
    config.baseUrl,
    config.base_url,
    appId === "workbuddy" ? config.url : undefined,
    config.options?.baseURL,
    config.env?.ANTHROPIC_BASE_URL,
    config.env?.GOOGLE_GEMINI_BASE_URL,
  ]);
}

function hasProviderCredential(provider: Provider): boolean {
  const config = provider.settingsConfig ?? {};
  return Boolean(
    getFirstString([
      config.apiKey,
      config.api_key,
      config.auth?.OPENAI_API_KEY,
      config.env?.ANTHROPIC_API_KEY,
      config.env?.OPENAI_API_KEY,
      config.env?.GEMINI_API_KEY,
    ]),
  );
}

function providerSort(a: Provider, b: Provider): number {
  return (
    (a.sortIndex ?? Number.MAX_SAFE_INTEGER) -
      (b.sortIndex ?? Number.MAX_SAFE_INTEGER) ||
    (a.createdAt ?? 0) - (b.createdAt ?? 0) ||
    a.name.localeCompare(b.name)
  );
}

function SecurityStatusRow({
  appType,
  onOpenSecurity,
  onOpenRoutingSettings,
}: {
  appType: AppId;
  onOpenSecurity: () => void;
  onOpenRoutingSettings: () => void;
}) {
  const { t, i18n } = useTranslation();
  const protection = useSecurityProtection(appType);
  const { startDate } = resolveUsageRange({ preset: "today" });
  const startTimestamp = startDate * 1000;
  const summary = useSecurityEventsSummary({
    view: "security_events",
    appType,
    startTimestamp,
  });

  const isLoading = protection.isLoading;
  const isProtected =
    protection.settings?.enabled === true && protection.connected;
  const displayCount = (value: number | undefined, pending: boolean) => {
    if (pending) return "…";
    if (value === undefined) return "—";
    return value.toLocaleString(i18n.language);
  };
  const handledCount = summary.data
    ? summary.data.protectiveBlocked + summary.data.redacted
    : undefined;
  const handledDisplay = summary.isError
    ? "—"
    : displayCount(handledCount, summary.isLoading);
  const warnedDisplay = summary.isError
    ? "—"
    : displayCount(summary.data?.warnings, summary.isLoading);
  const actions = (
    <div className="flex shrink-0 flex-wrap items-center justify-end gap-4">
      {protection.waiting && (
        <button
          type="button"
          disabled={protection.isUpdating}
          onClick={() => {
            void protection.setAppEnabled(false).catch(() => {});
          }}
          className="text-sm font-medium text-rose-600 hover:underline disabled:opacity-50"
        >
          {t(
            appType === "doubaowork"
              ? "homeAudit.doubaoStopWaiting"
              : "homeAudit.traeStopWaiting",
          )}
        </button>
      )}
      {!isProtected &&
        !protection.waiting &&
        (isTakeoverAppId(appType) ||
        (protection.supported && protection.connected) ? (
          <button
            type="button"
            disabled={protection.isUpdating}
            onClick={() => {
              void protection.setAppEnabled(true).catch(() => {});
            }}
            className="flex items-center gap-1 text-sm font-bold text-rose-600 hover:underline disabled:opacity-50 dark:text-rose-300"
          >
            {t(
              protection.connected
                ? "homeAudit.activate"
                : "homeAudit.enableRouting",
            )}
            <ChevronRight className="h-4 w-4" />
          </button>
        ) : (
          <button
            type="button"
            onClick={onOpenRoutingSettings}
            className="text-sm font-medium text-blue-600 hover:underline"
          >
            {t("homeAudit.configureRouting")}
          </button>
        ))}
      <button
        type="button"
        onClick={onOpenSecurity}
        className="flex items-center gap-1 text-sm font-medium text-blue-600 transition-colors hover:text-blue-700 hover:underline dark:text-blue-400"
      >
        {t("homeAudit.viewLogs")}
        <ChevronRight className="h-4 w-4" />
      </button>
    </div>
  );

  if (isLoading) {
    return (
      <section
        aria-label={t("homeAudit.label")}
        className="flex min-h-14 items-center justify-between rounded-2xl border border-slate-200/90 bg-white px-5 py-3.5 shadow-sm dark:border-border dark:bg-card"
      >
        <div className="h-4 w-64 animate-pulse rounded bg-slate-100 dark:bg-muted" />
        <div className="h-4 w-16 animate-pulse rounded bg-slate-100 dark:bg-muted" />
      </section>
    );
  }

  if (isProtected) {
    return (
      <section
        aria-label={t("homeAudit.label")}
        className="flex items-center justify-between gap-4 rounded-2xl border border-slate-200/90 bg-white px-5 py-3.5 shadow-sm transition-colors hover:border-slate-300 dark:border-border dark:bg-card"
      >
        <div className="flex min-w-0 items-center gap-2.5 text-sm text-slate-800 dark:text-foreground">
          <span className="relative flex h-2 w-2 shrink-0">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75" />
            <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-500" />
          </span>
          <span className="truncate">
            {t("homeAudit.auditSummary", {
              handled: handledDisplay,
              warned: warnedDisplay,
            })}
          </span>
        </div>
        {actions}
      </section>
    );
  }

  return (
    <section
      aria-label={t("homeAudit.label")}
      className="flex items-center justify-between gap-4 rounded-2xl border border-rose-300/90 bg-rose-50/80 px-5 py-3.5 shadow-sm transition-colors hover:border-rose-400 dark:border-rose-900/60 dark:bg-rose-950/20"
    >
      <div className="flex min-w-0 items-center gap-2.5 text-sm text-rose-900 dark:text-rose-200">
        <span className="relative flex h-2.5 w-2.5 shrink-0">
          <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-rose-400 opacity-75" />
          <span className="relative inline-flex h-2.5 w-2.5 rounded-full bg-rose-600" />
        </span>
        <AlertTriangle className="h-4 w-4 shrink-0 text-rose-600" />
        <span className="truncate font-medium">
          {t(
            protection.waiting
              ? appType === "doubaowork"
                ? "homeAudit.doubaoWaiting"
                : "homeAudit.traeWaiting"
              : !protection.connected
                ? "homeAudit.notConnected"
                : "homeAudit.disabled",
          )}
        </span>
      </div>
      {actions}
    </section>
  );
}

function ProviderIdentity({
  provider,
  appId,
  showEndpoint,
}: {
  provider: Provider;
  appId: AppId;
  showEndpoint?: boolean;
}) {
  const { t } = useTranslation();
  const model = getProviderModel(provider, appId);
  const endpoint = getProviderEndpoint(provider, appId);
  const credentialStatus =
    ["workbuddy", "qoder", "trae", "kimi", "qwen", "zcode"].includes(appId) &&
    provider.category === "official"
      ? t("desktopModels.officialLogin")
      : hasProviderCredential(provider)
        ? t("homeAudit.credentialConfigured")
        : resolveCodexOfficialIdentity(appId, provider) === "native_login"
          ? t("homeAudit.followCodexLogin")
          : t("homeAudit.noCredential");

  return (
    <div className="flex min-w-0 items-center gap-3.5">
      <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl border border-slate-200/80 bg-slate-50 shadow-sm dark:border-border dark:bg-muted/50">
        <ProviderIcon
          icon={resolveProviderIcon(appId, provider.icon, provider.iconColor)}
          name={provider.name}
          color={provider.iconColor}
          size={23}
        />
      </div>
      <div className="min-w-0 space-y-1">
        <div className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-1">
          <span className="truncate text-base font-bold tracking-tight text-slate-900 dark:text-foreground">
            {provider.name}
          </span>
          {provider.notes && (
            <span className="truncate text-sm font-normal text-slate-400 dark:text-muted-foreground">
              {provider.notes}
            </span>
          )}
        </div>
        <div className="flex min-w-0 flex-wrap items-center gap-2 text-xs text-slate-500 dark:text-muted-foreground sm:text-sm">
          {model && (
            <span className="truncate font-medium text-slate-600 dark:text-slate-300">
              {t("homeAudit.model")}: {model}
            </span>
          )}
          {model && <span className="font-bold text-slate-300">•</span>}
          <span className="inline-flex min-w-0 items-center gap-1 truncate">
            <LockKeyhole className="h-3 w-3 shrink-0 text-slate-400" />
            {credentialStatus}
          </span>
          {showEndpoint && endpoint && (
            <>
              <span className="font-bold text-slate-300">•</span>
              <span className="max-w-[18rem] truncate font-mono text-xs text-slate-400">
                {endpoint}
              </span>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

export function HomeDashboard({
  activeApp,
  projectSwitcher,
  visibleApps,
  providers,
  currentProviderId,
  activeProviderId,
  isLoading = false,
  onAppChange,
  onCreate,
  onEdit,
  onDelete,
  onRemoveFromConfig,
  onDuplicate,
  onSwitch,
  onOpenSecurity,
  onOpenRoutingSettings,
  onManageAgents,
}: HomeDashboardProps) {
  const { t } = useTranslation();
  const [traeNoticeExpanded, setTraeNoticeExpanded] = useState(false);
  const traeNoticeId = useId();
  const orderedProviders = useMemo(
    () => Object.values(providers).sort(providerSort),
    [providers],
  );
  const activeProvider =
    (activeProviderId ? providers[activeProviderId] : undefined) ||
    (currentProviderId ? providers[currentProviderId] : undefined);
  const alternateProviders = orderedProviders.filter(
    (provider) => provider.id !== activeProvider?.id,
  );

  return (
    <div
      data-testid="home-dashboard"
      className="mx-auto w-full max-w-none space-y-6 px-5 py-7 sm:px-8"
    >
      <section
        className="flex min-w-0 items-center gap-3.5"
        aria-label={t("homeAudit.managingApps")}
      >
        <span className="shrink-0 select-none text-sm font-medium text-slate-500 dark:text-muted-foreground">
          {t("homeAudit.managingApps")}
        </span>
        <ManagedAppTabs
          activeApp={activeApp}
          visibleApps={visibleApps}
          onAppChange={onAppChange}
        />
        {projectSwitcher}
        {onManageAgents && (
          <button
            type="button"
            onClick={onManageAgents}
            className="inline-flex shrink-0 items-center gap-1 text-sm text-blue-600 hover:text-blue-500"
          >
            <Plus className="h-4 w-4" />
            {t("homeAudit.manageAgents")}
          </button>
        )}
      </section>

      <AnimatePresence initial={false} mode="wait">
        <motion.div
          key={activeApp}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          className="space-y-6"
        >
          <SecurityStatusRow
            appType={activeApp}
            onOpenSecurity={onOpenSecurity}
            onOpenRoutingSettings={onOpenRoutingSettings}
          />
          {activeApp === "pi" ? (
            <ProviderList
              appId="pi"
              providers={providers}
              currentProviderId=""
              isLoading={isLoading}
              onSwitch={onSwitch}
              onEdit={onEdit}
              onDelete={onDelete}
              onRemoveFromConfig={onRemoveFromConfig}
              onDuplicate={onDuplicate}
              onOpenWebsite={settingsApi.openExternal}
            />
          ) : (
            <>
              {activeApp === "trae" && (
                <aside
                  role="note"
                  aria-label={t("homeAudit.traeNoticeTitle")}
                  className="rounded-2xl border border-amber-200 bg-amber-50/80 px-5 py-3 text-sm leading-relaxed text-amber-950 dark:border-amber-900/60 dark:bg-amber-950/20 dark:text-amber-100"
                >
                  <button
                    type="button"
                    aria-expanded={traeNoticeExpanded}
                    aria-controls={traeNoticeId}
                    onClick={() =>
                      setTraeNoticeExpanded((expanded) => !expanded)
                    }
                    className="flex w-full items-center gap-2 rounded-sm text-left font-semibold focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500"
                  >
                    <AlertTriangle className="h-4 w-4 shrink-0" />
                    {t("homeAudit.traeNoticeTitle")}
                    <ChevronRight
                      aria-hidden="true"
                      className={cn(
                        "ml-auto h-4 w-4 shrink-0 transition-transform",
                        traeNoticeExpanded && "rotate-90",
                      )}
                    />
                  </button>
                  <div
                    id={traeNoticeId}
                    hidden={!traeNoticeExpanded}
                    className="mt-3 space-y-2 border-t border-amber-200 pt-3 dark:border-amber-900/60"
                  >
                    <p className="font-medium">
                      {t("homeAudit.traeNoticeScope")}
                    </p>
                    <p>{t("homeAudit.traeNoticeConnection")}</p>
                  </div>
                </aside>
              )}
              {activeApp === "qoder" && <QoderCertificatePanel />}

              {activeApp === "doubaowork" ? (
                <DoubaoWorkPanel />
              ) : isLoading ? (
                <section
                  className="space-y-3"
                  aria-label={t("homeAudit.activeSet")}
                >
                  <div className="h-5 w-48 animate-pulse rounded bg-slate-100 dark:bg-muted" />
                  <div className="h-28 animate-pulse rounded-2xl bg-white shadow-sm dark:bg-card" />
                </section>
              ) : activeProvider ? (
                <section className="space-y-2.5">
                  <div className="flex items-center justify-between gap-3">
                    <h2 className="select-none text-sm font-medium text-slate-500 dark:text-muted-foreground">
                      {t("homeAudit.activeSet", {
                        app: t(`apps.${activeApp}`),
                      })}
                    </h2>
                    <span className="shrink-0 font-mono text-xs text-slate-400 dark:text-muted-foreground">
                      {t("homeAudit.gatewayPort", { port: LOCAL_PROXY_PORT })}
                    </span>
                  </div>
                  <div className="relative overflow-hidden rounded-2xl border-2 border-blue-600 bg-white p-5 shadow-sm shadow-blue-500/10 transition-all dark:bg-card sm:p-6">
                    <div className="relative z-10 flex flex-col justify-between gap-4 sm:flex-row sm:items-center">
                      <ProviderIdentity
                        provider={activeProvider}
                        appId={activeApp}
                      />
                      <div className="flex shrink-0 items-center gap-2.5 self-start sm:self-center">
                        <span className="inline-flex items-center gap-1.5 rounded-lg border border-blue-100 bg-blue-50 px-3 py-1.5 text-xs font-semibold text-blue-600 dark:border-blue-500/30 dark:bg-blue-950/30 dark:text-blue-300">
                          <span className="inline-block h-1.5 w-1.5 rounded-full bg-blue-600" />
                          {t("provider.inUse")}
                        </span>
                        <button
                          type="button"
                          onClick={() => onEdit(activeProvider)}
                          className="rounded-lg border border-slate-200 bg-white px-4 py-1.5 text-xs font-medium text-slate-700 shadow-sm transition-all hover:border-slate-300 hover:bg-slate-50 dark:border-border dark:bg-card dark:text-slate-200 dark:hover:bg-muted"
                        >
                          {t("common.edit")}
                        </button>
                        <button
                          type="button"
                          onClick={() => onDuplicate(activeProvider)}
                          title={t("provider.duplicate")}
                          aria-label={t("provider.duplicate")}
                          className="rounded-lg border border-slate-200 p-2 text-slate-500 transition-all hover:border-slate-300 hover:bg-slate-50 hover:text-slate-900 dark:border-border dark:hover:bg-muted dark:hover:text-foreground"
                        >
                          <Copy className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    </div>
                  </div>
                </section>
              ) : (
                <section className="space-y-3 rounded-2xl border-2 border-dashed border-slate-200 bg-white p-8 text-center shadow-sm dark:border-border dark:bg-card">
                  <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl border border-blue-100 bg-blue-50 text-blue-600 dark:border-blue-500/30 dark:bg-blue-950/30">
                    <Plus className="h-5 w-5" />
                  </div>
                  <div>
                    <h2 className="text-base font-bold text-slate-900 dark:text-foreground">
                      {t(
                        orderedProviders.length > 0
                          ? "homeAudit.noCurrentProviderForApp"
                          : "homeAudit.noProvidersForApp",
                        {
                          app: t(`apps.${activeApp}`),
                        },
                      )}
                    </h2>
                    <p className="mt-1 text-xs text-slate-500 dark:text-muted-foreground">
                      {t(
                        orderedProviders.length > 0
                          ? "homeAudit.selectProviderHint"
                          : "provider.addProviderHint",
                      )}
                    </p>
                  </div>
                  <button
                    type="button"
                    onClick={onCreate}
                    className="inline-flex items-center gap-1.5 rounded-xl bg-blue-600 px-4 py-2 text-xs font-semibold text-white shadow-sm transition-colors hover:bg-blue-700"
                  >
                    <Plus className="h-4 w-4" />
                    {t(
                      orderedProviders.length > 0
                        ? "provider.addProvider"
                        : "homeAudit.addFirstProvider",
                    )}
                  </button>
                </section>
              )}

              {activeApp !== "doubaowork" && (
                <section className="space-y-2.5 pt-2">
                  <div className="flex items-center justify-between gap-3 select-none">
                    <div className="flex items-center gap-2">
                      <h2 className="text-sm font-medium text-slate-500 dark:text-muted-foreground">
                        {t("homeAudit.alternatives")}
                      </h2>
                      <span className="text-xs text-slate-400 dark:text-muted-foreground">
                        {t("homeAudit.alternativeCount", {
                          count: alternateProviders.length,
                        })}
                      </span>
                    </div>
                    <div className="flex shrink-0 items-center gap-3">
                      <button
                        type="button"
                        onClick={onCreate}
                        className="flex shrink-0 items-center gap-1 text-xs font-medium text-blue-600 transition-colors hover:text-blue-700 dark:text-blue-400"
                      >
                        <Plus className="h-3.5 w-3.5" />
                        {t("homeAudit.addAlternative")}
                      </button>
                    </div>
                  </div>

                  {alternateProviders.length > 0 ? (
                    <div className="space-y-3">
                      {alternateProviders.map((provider) => (
                        <div
                          key={provider.id}
                          className="group flex flex-col justify-between gap-3 rounded-2xl border border-slate-200/90 bg-white p-4 shadow-sm transition-all hover:border-slate-300 hover:shadow-md dark:border-border dark:bg-card sm:flex-row sm:items-center sm:p-5"
                        >
                          <ProviderIdentity
                            provider={provider}
                            appId={activeApp}
                            showEndpoint
                          />
                          <div className="flex shrink-0 items-center gap-2 self-end sm:self-center">
                            <button
                              type="button"
                              onClick={() => onDuplicate(provider)}
                              title={t("provider.duplicate")}
                              aria-label={`${t("provider.duplicate")} ${provider.name}`}
                              className="rounded-lg border border-slate-200 bg-white p-2 text-slate-500 shadow-sm transition-all hover:border-slate-300 hover:bg-slate-50 hover:text-slate-900 dark:border-border dark:bg-card dark:hover:bg-muted dark:hover:text-foreground"
                            >
                              <Copy className="h-3.5 w-3.5" />
                            </button>
                            <button
                              type="button"
                              onClick={() => onEdit(provider)}
                              title={t("common.edit")}
                              aria-label={`${t("common.edit")} ${provider.name}`}
                              className="rounded-lg border border-slate-200 bg-white p-2 text-slate-500 shadow-sm transition-all hover:border-slate-300 hover:bg-slate-50 hover:text-slate-900 dark:border-border dark:bg-card dark:hover:bg-muted dark:hover:text-foreground"
                            >
                              <Edit2 className="h-3.5 w-3.5" />
                            </button>
                            <button
                              type="button"
                              onClick={() => onDelete(provider)}
                              title={t("common.delete")}
                              aria-label={`${t("common.delete")} ${provider.name}`}
                              className="rounded-lg border border-slate-200 bg-white p-2 text-slate-500 shadow-sm transition-all hover:border-rose-300 hover:bg-rose-50 hover:text-rose-600 dark:border-border dark:bg-card dark:hover:border-rose-500/40 dark:hover:bg-rose-950/30 dark:hover:text-rose-400"
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </button>
                            <button
                              type="button"
                              onClick={() => onSwitch(provider)}
                              className="rounded-lg border border-slate-200 bg-white px-4 py-1.5 text-xs font-semibold text-slate-700 shadow-sm transition-all hover:border-blue-600 hover:bg-blue-50/50 hover:text-blue-600 dark:border-border dark:bg-card dark:text-slate-200 dark:hover:bg-blue-950/30"
                            >
                              {t("homeAudit.switchTo")}
                            </button>
                          </div>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="rounded-xl border border-slate-200/70 bg-slate-50/60 py-4 text-center text-xs text-slate-400 dark:border-border dark:bg-muted/30 dark:text-muted-foreground">
                      {t("homeAudit.noAlternatives")}
                    </div>
                  )}
                </section>
              )}
            </>
          )}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
