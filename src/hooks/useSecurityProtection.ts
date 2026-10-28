import {
  useIsMutating,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useProxyStatusQuery } from "@/lib/query/proxy";
import { proxyApi } from "@/lib/api/proxy";
import { providersApi, type ClaudeDesktopStatus } from "@/lib/api/providers";
import { securityApi } from "@/lib/api/security";
import {
  APP_IDS,
  DEFAULT_VISIBLE_APPS,
  isRoutingAppId,
} from "@/config/appConfig";
import { piApi } from "@/lib/api/pi";
import { settingsApi } from "@/lib/api/settings";
import { useSecuritySettings } from "./useSecurityAudit";
import { extractErrorMessage } from "@/utils/errorUtils";

function needsQwenSetup(app: string, error: unknown) {
  return app === "qwen" && extractErrorMessage(error) === "homeAudit.qwenSetup";
}

function desktopConfigured(s: ClaudeDesktopStatus) {
  return (
    s.supported &&
    s.configured &&
    s.mode === "proxy" &&
    !!s.expectedBaseUrl &&
    s.actualBaseUrl === s.expectedBaseUrl &&
    s.gatewayTokenConfigured &&
    !s.missingRouteMappings &&
    !s.staleRawModels
  );
}
async function routeState(
  app: string,
): Promise<{ connected: boolean; waiting: boolean }> {
  if (app === "pi") {
    const status = await piApi.getRoutingStatus();
    return {
      connected:
        status.enabled &&
        status.configured &&
        status.running &&
        !status.errorCode,
      waiting: false,
    };
  }
  if (app === "claude-desktop") {
    const status = await providersApi.getClaudeDesktopStatus();
    return {
      connected: desktopConfigured(status) && status.proxyRunning,
      waiting: false,
    };
  }
  if (!isRoutingAppId(app)) return { connected: false, waiting: false };
  const [status, takeover] = await Promise.all([
    proxyApi.getProxyStatus(),
    proxyApi.getProxyTakeoverStatus(),
  ]);
  return {
    connected: status.running && !!takeover[app],
    waiting:
      (app === "trae" && takeover.trae_waiting === true) ||
      (app === "doubaowork" && takeover.doubaowork_waiting === true),
  };
}
async function routeReady(app: string) {
  return (await routeState(app)).connected;
}
export async function ensureSecurityRoute(app: string) {
  if (app === "claude-desktop") {
    const status = await providersApi.getClaudeDesktopStatus();
    if (!desktopConfigured(status)) throw new Error("homeAudit.desktopSetup");
    if (!status.proxyRunning) await proxyApi.startProxyServer();
  } else {
    if (!isRoutingAppId(app)) throw new Error("homeAudit.unsupported");
    if (!(await routeReady(app)))
      await proxyApi.setProxyTakeoverForApp(app, true);
  }
  const state = await routeState(app);
  if (!state.connected && !state.waiting)
    throw new Error("homeAudit.routeFailed");
}
export async function ensureAddedSecurityRoutes() {
  const [settings, installed] = await Promise.all([
    settingsApi.get(),
    proxyApi.getAgentInstallations(),
  ]);
  const added = { ...DEFAULT_VISIBLE_APPS, ...settings.visibleApps };
  const result: { activated: string[]; skipped: string[]; failed: string[] } = {
    activated: [],
    skipped: [],
    failed: [],
  };
  for (const app of APP_IDS) {
    if (!added[app] || !installed[app]) continue;
    if (app !== "claude-desktop" && !isRoutingAppId(app)) continue;
    try {
      if (app === "claude-desktop") {
        // Installing Desktop does not imply the user configured its gateway mapping.
        if (!desktopConfigured(await providersApi.getClaudeDesktopStatus())) {
          result.skipped.push(app);
          continue;
        }
      } else if (app === "pi") {
        // Pi routes explicit saved models; it has no Vary-owned native default selection.
        const providers = await providersApi.getAll("pi");
        if (!Object.keys(providers).some((id) => id !== "vary")) {
          result.skipped.push(app);
          continue;
        }
      } else if (app !== "doubaowork") {
        const [current, providers] = await Promise.all([
          providersApi.getCurrent(app),
          providersApi.getAll(app),
        ]);
        if (!current || !providers[current]) {
          result.skipped.push(app);
          continue;
        }
      }
      await ensureSecurityRoute(app);
      result.activated.push(app);
    } catch (error) {
      if (needsQwenSetup(app, error)) result.skipped.push(app);
      else result.failed.push(app);
    }
  }
  return result;
}
export async function setCurrentAppProtection(app: string, enabled: boolean) {
  if (!isRoutingAppId(app) && !(app === "claude-desktop" && enabled))
    throw new Error("homeAudit.unsupported");
  if (enabled) {
    await ensureSecurityRoute(app);
    const settings = await securityApi.getSettings();
    if (!settings.enabled)
      await securityApi.updateSettings({ ...settings, enabled: true });
  } else {
    if (isRoutingAppId(app)) await proxyApi.setProxyTakeoverForApp(app, false);
  }
}

export function useSecurityProtection(app: string) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const { settings, isLoading } = useSecuritySettings();
  const proxyStatus = useProxyStatusQuery();
  const route = useQuery({
    queryKey: ["security-route", app],
    queryFn: () => routeState(app),
    refetchInterval: 2000,
  });
  const busy = useIsMutating({ mutationKey: ["security-protection"] }) > 0;
  const routeBusy =
    useIsMutating({ mutationKey: ["security-route-toggle"] }) > 0;
  const routeMutation = useMutation({
    mutationKey: ["security-route-toggle", app],
    mutationFn: (enabled: boolean) => setCurrentAppProtection(app, enabled),
    onSuccess: async (_data, enabled) => {
      if (
        enabled &&
        ["trae", "doubaowork"].includes(app) &&
        (await routeState(app)).waiting
      ) {
        toast.info(
          t(
            app === "doubaowork"
              ? "homeAudit.doubaoWaiting"
              : "homeAudit.traeWaiting",
          ),
          {
            closeButton: true,
            description: t("homeAudit.newConversationNotice"),
          },
        );
        return;
      }
      toast.success(
        t(
          app === "pi"
            ? enabled
              ? "piRouting.enabled"
              : "piRouting.disabled"
            : enabled
              ? "homeAudit.appProtectionEnabled"
              : "homeAudit.appProtectionDisabled",
          { app: t(`apps.${app}`) },
        ),
        {
          closeButton: true,
          description:
            [
              app === "qoder" || app === "workbuddy" || app === "kimi"
                ? t("homeAudit.appProtectionRestart", { app: t(`apps.${app}`) })
                : undefined,
              enabled ? t("homeAudit.newConversationNotice") : undefined,
            ]
              .filter(Boolean)
              .join(" ") || undefined,
        },
      );
    },
    onError: (error) => {
      if (needsQwenSetup(app, error)) {
        toast.info(t("homeAudit.qwenSetup"));
      } else {
        toast.error(t("homeAudit.appProtectionFailed"));
      }
    },
    onSettled: async () => {
      await client.invalidateQueries();
    },
  });
  const mutation = useMutation({
    mutationKey: ["security-protection"],
    mutationFn: async (enabled: boolean) => {
      const routes = enabled ? await ensureAddedSecurityRoutes() : undefined;
      const current = await securityApi.getSettings();
      await securityApi.updateSettings({ ...current, enabled });
      return routes;
    },
    onSuccess: (routes, enabled) => {
      if (enabled) {
        const nextSteps = [
          ...(routes?.activated ?? [])
            .filter((app) => ["qoder", "workbuddy", "kimi"].includes(app))
            .map((app) =>
              t("homeAudit.appProtectionRestart", { app: t(`apps.${app}`) }),
            ),
          t("homeAudit.newConversationNotice"),
        ].join(" ");
        if (routes && (routes.failed.length || routes.skipped.length)) {
          const labels = (apps: string[]) =>
            apps.map((app) => t(`apps.${app}`)).join(", ") || "—";
          toast.warning(t("homeAudit.batchPartial"), {
            description:
              t("homeAudit.batchDetails", {
                count: routes.activated.length,
                skipped: labels(routes.skipped),
                failed: labels(routes.failed),
              }) +
              " " +
              nextSteps,
            closeButton: true,
          });
          return;
        }
        toast.success(t("homeAudit.enabled"), {
          description: t("homeAudit.protectionDetail") + " " + nextSteps,
        });
      } else {
        toast.warning(t("homeAudit.disabled"), {
          description: t("homeAudit.disabledDetail"),
        });
      }
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(
        t("homeAudit.enableFailed") +
          ": " +
          (message.startsWith("homeAudit.") ? t(message) : message),
      );
    },
    onSettled: async () => {
      await client.invalidateQueries();
    },
  });
  return {
    settings: settings
      ? {
          ...settings,
          enabled:
            settings.enabled &&
            proxyStatus.data?.running === true &&
            !proxyStatus.isError,
        }
      : settings,
    connected: route.data?.connected === true && !route.isError,
    waiting: route.data?.waiting === true && !route.isError,
    isLoading: isLoading || route.isPending || proxyStatus.isPending,
    isUpdating: busy || routeBusy,
    supported: isRoutingAppId(app) || app === "claude-desktop",
    setEnabled: mutation.mutateAsync,
    setAppEnabled: routeMutation.mutateAsync,
  };
}
