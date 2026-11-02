import { ConnectionPresetPicker } from "./ConnectionPresetPicker";
import { ProviderConfigDetection } from "./ProviderConfigDetection";
import { useEffect, useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Download, Loader2, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { BasicFormFields } from "./BasicFormFields";
import { CodexOAuthSection } from "./CodexOAuthSection";
import { CopilotAuthSection } from "./CopilotAuthSection";
import { XaiOAuthSection } from "./XaiOAuthSection";
import { ApiKeySection } from "./shared/ApiKeySection";
import { EndpointField } from "./shared/EndpointField";
import { ModelDropdown } from "./shared/ModelDropdown";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { useApiKeyLink } from "./hooks/useApiKeyLink";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import type {
  ClaudeApiFormat,
  ClaudeDesktopModelRoute,
  ProviderCategory,
  ProviderMeta,
} from "@/types";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import {
  CLAUDE_DESKTOP_ROLE_ROUTE_IDS,
  claudeDesktopProviderPresets,
  type ClaudeDesktopProviderPreset,
  type ClaudeDesktopRoleId,
} from "@/config/claudeDesktopProviderPresets";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import {
  providersApi,
  type ClaudeDesktopDefaultRoute,
} from "@/lib/api/providers";
import { resolveManagedAccountId } from "@/lib/authBinding";
import type { ManagedAuthProvider } from "@/lib/api";
import { useCopilotAuth, useCodexOauth, useXaiOauth } from "./hooks";
import { isOAuthProviderType } from "@/config/constants";

export type ClaudeDesktopProviderFormValues = ProviderFormData & {
  presetId?: string;
  presetCategory?: ProviderCategory;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  meta?: ProviderMeta;
  providerKey?: string;
  suggestedDefaults?: OpenClawSuggestedDefaults;
};

type ApiKeyField = "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY";

type PresetEntry = {
  id: string;
  preset: ClaudeDesktopProviderPreset;
};

export interface ClaudeDesktopProviderFormProps {
  submitLabel: string;
  onSubmit: (values: ClaudeDesktopProviderFormValues) => Promise<void> | void;
  onCancel: () => void;
  onSubmittingChange?: (isSubmitting: boolean) => void;
  initialData?: {
    name?: string;
    websiteUrl?: string;
    notes?: string;
    settingsConfig?: Record<string, unknown>;
    category?: ProviderCategory;
    meta?: ProviderMeta;
    icon?: string;
    iconColor?: string;
  };
  showButtons?: boolean;
  onManageAuthAccounts?: (target: ManagedAuthProvider) => void;
}

type RouteRow = {
  rowId: string;
  route: string;
  model: string;
  labelOverride: string;
  supports1m: boolean;
};

type RouteRowValues = Omit<RouteRow, "rowId">;
type RouteRole = ClaudeDesktopRoleId;

const CLAUDE_ROUTE_PREFIX = "claude-";
const ANTHROPIC_CLAUDE_ROUTE_PREFIX = "anthropic/claude-";
const LEGACY_ONE_M_MARKER = "[1m]";
const ROLE_ROUTE_IDS = CLAUDE_DESKTOP_ROLE_ROUTE_IDS;
const ROLE_ORDER: RouteRole[] = ["sonnet", "opus", "fable", "haiku"];

function envString(
  settingsConfig: Record<string, unknown> | undefined,
  key: string,
) {
  const env = settingsConfig?.env;
  if (!env || typeof env !== "object") return "";
  const value = (env as Record<string, unknown>)[key];
  return typeof value === "string" ? value : "";
}

function clonePlainRecord(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  return { ...(value as Record<string, unknown>) };
}

function routeRoleFromId(route: string): RouteRole {
  const normalized = route.trim().toLowerCase();
  // 与后端 claude_role_keyword 同序（opus → haiku → fable → sonnet）。
  if (normalized.includes("opus")) return "opus";
  if (normalized.includes("haiku")) return "haiku";
  if (normalized.includes("fable")) return "fable";
  return "sonnet";
}

function routeIdForRole(role: RouteRole, usedRoutes: Set<string>) {
  const baseRoute = ROLE_ROUTE_IDS[role];
  if (!usedRoutes.has(baseRoute)) return baseRoute;

  let index = 2;
  while (usedRoutes.has(`${baseRoute}-r${index}`)) {
    index += 1;
  }
  return `${baseRoute}-r${index}`;
}

function fallbackCatalogRouteId(usedRoutes: Set<string>) {
  const role = ROLE_ORDER.find((candidate) => {
    const route = ROLE_ROUTE_IDS[candidate];
    return !usedRoutes.has(route);
  });
  return routeIdForRole(role ?? "sonnet", usedRoutes);
}

function createRouteRow(row: RouteRowValues): RouteRow {
  return {
    rowId: crypto.randomUUID(),
    ...row,
  };
}

function initialRouteRows(
  routes: Record<string, ClaudeDesktopModelRoute> | undefined,
): RouteRow[] {
  const usedRoutes = new Set(
    Object.keys(routes ?? {}).filter((route) => isClaudeSafeRoute(route)),
  );

  return Object.entries(routes ?? {}).map(([route, value]) => {
    const routeId = isClaudeSafeRoute(route)
      ? route
      : fallbackCatalogRouteId(usedRoutes);
    usedRoutes.add(routeId);

    return createRouteRow({
      route: routeId,
      model: value.model ?? "",
      labelOverride:
        value.labelOverride ??
        (!isClaudeSafeRoute(route) ? value.model || route : ""),
      supports1m: value.supports1m ?? false,
    });
  });
}

function isClaudeSafeRoute(route: string) {
  const normalized = route.trim().toLowerCase();
  if (normalized.includes(LEGACY_ONE_M_MARKER)) return false;
  const routeTail = normalized.startsWith(ANTHROPIC_CLAUDE_ROUTE_PREFIX)
    ? normalized.slice(ANTHROPIC_CLAUDE_ROUTE_PREFIX.length)
    : normalized.startsWith(CLAUDE_ROUTE_PREFIX)
      ? normalized.slice(CLAUDE_ROUTE_PREFIX.length)
      : "";

  // 角色前缀后必须还有实际模型标识，拒绝 claude-sonnet- 这类退化值
  // （否则会写入 profile 并触发 Claude Desktop fail-all 拒收整组）。
  // 与后端 is_claude_safe_model_id 镜像；fable 自 Desktop 1.12603.1+ 起被校验放行。
  return ["sonnet-", "opus-", "haiku-", "fable-"].some(
    (prefix) =>
      routeTail.startsWith(prefix) && routeTail.length > prefix.length,
  );
}

function defaultRouteRows(
  defaults: ClaudeDesktopDefaultRoute[],
  defaultModel: string,
): RouteRow[] {
  return defaults.map((route, index) =>
    createRouteRow({
      route: route.routeId,
      model: index === 0 ? defaultModel : "",
      labelOverride: index === 0 ? defaultModel : "",
      supports1m: route.supports1m,
    }),
  );
}

export function ClaudeDesktopProviderForm({
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  initialData,
  showButtons = true,
  onManageAuthAccounts,
}: ClaudeDesktopProviderFormProps) {
  const { t } = useTranslation();
  const [apiFormat, setApiFormat] = useState<ClaudeApiFormat>(
    initialData?.meta?.apiFormat ?? "anthropic",
  );
  const [editingCredentials, setEditingCredentials] = useState(false);
  const [protocolManuallySelected, setProtocolManuallySelected] =
    useState(false);
  const [baseUrl, setBaseUrl] = useState(
    envString(initialData?.settingsConfig, "ANTHROPIC_BASE_URL"),
  );
  const [apiKey, setApiKey] = useState(
    envString(initialData?.settingsConfig, "ANTHROPIC_AUTH_TOKEN") ||
      envString(initialData?.settingsConfig, "ANTHROPIC_API_KEY"),
  );
  const [apiKeyField, setApiKeyField] = useState<ApiKeyField>(() =>
    envString(initialData?.settingsConfig, "ANTHROPIC_API_KEY")
      ? "ANTHROPIC_API_KEY"
      : "ANTHROPIC_AUTH_TOKEN",
  );
  const [selectedGitHubAccountId, setSelectedGitHubAccountId] = useState<
    string | null
  >(() => resolveManagedAccountId(initialData?.meta, "github_copilot"));
  const [selectedCodexAccountId, setSelectedCodexAccountId] = useState<
    string | null
  >(() => resolveManagedAccountId(initialData?.meta, "codex_oauth"));
  const [selectedXaiAccountId, setSelectedXaiAccountId] = useState<
    string | null
  >(() => resolveManagedAccountId(initialData?.meta, "xai_oauth"));
  const [codexFastMode, setCodexFastMode] = useState<boolean>(
    () => initialData?.meta?.codexFastMode ?? false,
  );
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    "custom",
  );
  const [activePreset, setActivePreset] = useState<{
    id: string;
    category?: ProviderCategory;
    isPartner?: boolean;
    partnerPromotionKey?: string;
    providerType?: string;
    requiresOAuth?: boolean;
  } | null>(null);
  const [proxyRoutes, setProxyRoutes] = useState<RouteRow[]>(() =>
    initialRouteRows(initialData?.meta?.claudeDesktopModelRoutes),
  );
  const didSeedDefaultProxyRoutes = useRef(
    Object.keys(initialData?.meta?.claudeDesktopModelRoutes ?? {}).length > 0,
  );
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const modelFetchSequence = useRef(0);

  useEffect(() => {
    modelFetchSequence.current += 1;
    setFetchedModels([]);
    setIsFetchingModels(false);
    return () => {
      modelFetchSequence.current += 1;
    };
  }, [baseUrl, apiKey]);
  const { data: defaultRoutes = [] } = useQuery({
    queryKey: ["claudeDesktopDefaultRoutes"],
    queryFn: () => providersApi.getClaudeDesktopDefaultRoutes(),
  });
  const defaultProxyRouteRows = useMemo(
    () =>
      defaultRouteRows(
        defaultRoutes,
        envString(initialData?.settingsConfig, "ANTHROPIC_MODEL"),
      ),
    [defaultRoutes, initialData?.settingsConfig],
  );

  const defaultValues: ProviderFormData = useMemo(
    () => ({
      name: initialData?.name ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: JSON.stringify(
        initialData?.settingsConfig ?? { env: {} },
        null,
        2,
      ),
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
    }),
    [initialData],
  );

  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues,
    mode: "onSubmit",
  });

  useEffect(() => {
    onSubmittingChange?.(form.formState.isSubmitting || isFetchingModels);
  }, [form.formState.isSubmitting, isFetchingModels, onSubmittingChange]);

  const presetEntries = useMemo<PresetEntry[]>(
    () =>
      claudeDesktopProviderPresets.map((preset, index) => ({
        id: `claude-desktop-${index}`,
        preset,
      })),
    [],
  );

  const presetCategoryLabels: Record<string, string> = useMemo(
    () => ({
      official: t("providerForm.categoryOfficial", { defaultValue: "官方" }),
      cn_official: t("providerForm.categoryCnOfficial", {
        defaultValue: "国内官方",
      }),
      aggregator: t("providerForm.categoryAggregation", {
        defaultValue: "聚合服务",
      }),
      third_party: t("providerForm.categoryThirdParty", {
        defaultValue: "第三方",
      }),
    }),
    [t],
  );
  const activeProviderType =
    activePreset?.providerType ?? initialData?.meta?.providerType;
  const { isAuthenticated: isCopilotAuthenticated, accounts: copilotAccounts } =
    useCopilotAuth();
  const {
    isAuthenticated: isCodexOauthAuthenticated,
    defaultAccountId: codexOauthDefaultAccountId,
    accounts: codexOauthAccounts,
  } = useCodexOauth();
  const {
    isAuthenticated: isXaiOauthAuthenticated,
    accounts: xaiOauthAccounts,
  } = useXaiOauth();
  const isOfficial =
    initialData?.category === "official" ||
    activePreset?.category === "official";
  const usesManagedOAuth =
    activePreset?.requiresOAuth === true ||
    isOAuthProviderType(activeProviderType);
  const routes = proxyRoutes;
  const setRoutes = setProxyRoutes;

  // API Key 获取/邀请链接（与 Claude Code 表单同款，见 ClaudeFormFields）
  const apiKeyLinkCategory = activePreset?.category ?? initialData?.category;
  const {
    shouldShowApiKeyLink,
    websiteUrl: apiKeyLinkWebsiteUrl,
    isPartner: apiKeyLinkIsPartner,
    partnerPromotionKey: apiKeyLinkPromotionKey,
  } = useApiKeyLink({
    appId: "claude-desktop",
    category: apiKeyLinkCategory,
    selectedPresetId,
    presetEntries,
    formWebsiteUrl: form.watch("websiteUrl") || "",
  });

  const applyDesktopPreset = (preset: ClaudeDesktopProviderPreset) => {
    form.setValue("name", preset.nameKey ? t(preset.nameKey) : preset.name);
    form.setValue("websiteUrl", preset.websiteUrl);
    form.setValue("notes", "");
    form.setValue("icon", preset.icon ?? "");
    form.setValue("iconColor", preset.iconColor ?? "");

    setBaseUrl(preset.baseUrl);
    setApiKey("");
    setApiKeyField(preset.apiKeyField ?? "ANTHROPIC_AUTH_TOKEN");
    setApiFormat(preset.apiFormat ?? "anthropic");

    if (preset.modelRoutes) {
      didSeedDefaultProxyRoutes.current = true;
      setProxyRoutes(
        preset.modelRoutes.map((row) =>
          createRouteRow({
            route: preset.mode === "direct" ? row.upstreamModel : row.routeId,
            model: row.upstreamModel,
            labelOverride: row.labelOverride ?? "",
            supports1m: row.supports1m,
          }),
        ),
      );
    } else {
      didSeedDefaultProxyRoutes.current = false;
      setProxyRoutes([]);
    }
  };

  const handlePresetChange = (value: string) => {
    setSelectedPresetId(value);

    if (value === "custom") {
      setActivePreset(null);
      form.reset(defaultValues);
      setBaseUrl("");
      setApiKey("");
      setApiKeyField("ANTHROPIC_AUTH_TOKEN");
      setApiFormat("anthropic");
      didSeedDefaultProxyRoutes.current = false;
      setProxyRoutes([]);
      return;
    }

    const entry = presetEntries.find((item) => item.id === value);
    if (!entry) return;

    setActivePreset({
      id: value,
      category: entry.preset.category,
      isPartner: entry.preset.isPartner,
      partnerPromotionKey: entry.preset.partnerPromotionKey,
      providerType: entry.preset.providerType,
      requiresOAuth: entry.preset.requiresOAuth,
    });
    applyDesktopPreset(entry.preset);
  };

  const updateRoute = (index: number, patch: Partial<RouteRowValues>) => {
    setRoutes((current) =>
      current.map((row, i) => (i === index ? { ...row, ...patch } : row)),
    );
  };

  const addProxyRoute = () => {
    didSeedDefaultProxyRoutes.current = true;
    setProxyRoutes((current) => [
      ...current,
      createRouteRow({
        route: fallbackCatalogRouteId(new Set(current.map((row) => row.route))),
        model: "",
        labelOverride: "",
        supports1m: false,
      }),
    ]);
  };

  useEffect(() => {
    if (
      didSeedDefaultProxyRoutes.current ||
      proxyRoutes.length > 0 ||
      defaultProxyRouteRows.length === 0
    ) {
      return;
    }

    didSeedDefaultProxyRoutes.current = true;
    setProxyRoutes(defaultProxyRouteRows);
  }, [defaultProxyRouteRows, proxyRoutes.length]);

  const handleFetchModels = async (importAll = false) => {
    if (!baseUrl.trim() || !apiKey.trim()) {
      showFetchModelsError(null, t, {
        hasBaseUrl: Boolean(baseUrl.trim()),
        hasApiKey: Boolean(apiKey.trim()),
      });
      return;
    }

    const sequence = ++modelFetchSequence.current;
    setIsFetchingModels(true);
    try {
      const models =
        fetchedModels.length > 0
          ? fetchedModels
          : await fetchModelsForConfig(
              baseUrl.trim(),
              apiKey.trim(),
              undefined,
              undefined,
              undefined,
              {
                apiFormat:
                  apiFormat === "gemini_native"
                    ? "google-generative-ai"
                    : undefined,
              },
            );
      if (sequence !== modelFetchSequence.current) return;
      setFetchedModels(models);
      const ids = [
        ...new Set(models.map(({ id }) => id.trim()).filter(Boolean)),
      ];
      if (ids.length) {
        if (importAll) didSeedDefaultProxyRoutes.current = true;
        setProxyRoutes((current) => {
          if (!importAll) {
            return current.map((row) =>
              ids.includes(row.model.trim())
                ? row
                : { ...row, model: ids[0], labelOverride: ids[0] },
            );
          }
          const rows = [...current];
          const knownModels = new Set(current.map((row) => row.model.trim()));
          const usedRoutes = new Set(current.map((row) => row.route));
          for (const model of ids) {
            if (knownModels.has(model)) continue;
            knownModels.add(model);
            const values = { model, labelOverride: model, supports1m: false };
            const emptyIndex = rows.findIndex((row) => !row.model.trim());
            if (emptyIndex >= 0) {
              rows[emptyIndex] = { ...rows[emptyIndex], ...values };
            } else {
              const route = fallbackCatalogRouteId(usedRoutes);
              usedRoutes.add(route);
              rows.push(createRouteRow({ route, ...values }));
            }
          }
          return rows;
        });
      }
      toast.success(
        t(
          importAll
            ? "providerForm.quickSetSuccess"
            : "providerForm.fetchModelsSuccess",
          {
            count: ids.length,
            defaultValue: importAll
              ? `Imported ${ids.length} models`
              : `已获取 ${ids.length} 个模型`,
          },
        ),
      );
    } catch (error) {
      if (sequence !== modelFetchSequence.current) return;
      showFetchModelsError(error, t, {
        hasBaseUrl: Boolean(baseUrl.trim()),
        hasApiKey: Boolean(apiKey.trim()),
      });
    } finally {
      if (sequence === modelFetchSequence.current) setIsFetchingModels(false);
    }
  };

  const handleSubmit = async (values: ProviderFormData) => {
    if (!values.name.trim()) {
      toast.error(
        t("providerForm.fillSupplierName", {
          defaultValue: "请填写供应商名称",
        }),
      );
      return;
    }
    if (isOfficial) {
      // 官方供应商使用 Claude Desktop 内置 1P 模式，保持空 env 占位；
      // 不写 claudeDesktopMode / claudeDesktopModelRoutes / apiFormat，
      // 与启动 seed 的 OFFICIAL_SEEDS 占位语义一致。
      const settingsConfig = clonePlainRecord(initialData?.settingsConfig);
      settingsConfig.env = {};
      const meta: ProviderMeta = { ...(initialData?.meta ?? {}) };
      delete meta.claudeDesktopMode;
      delete meta.claudeDesktopModelRoutes;
      delete meta.apiFormat;
      delete meta.endpointAutoSelect;
      delete meta.isFullUrl;
      await onSubmit({
        ...values,
        name: values.name.trim(),
        websiteUrl: values.websiteUrl?.trim() ?? "",
        notes: values.notes?.trim() ?? "",
        settingsConfig: JSON.stringify(settingsConfig, null, 2),
        meta,
        presetId: activePreset?.id,
        presetCategory: "official",
      });
      return;
    }
    if (!baseUrl.trim() && !usesManagedOAuth) {
      toast.error(
        t("providerForm.fetchModelsNeedEndpoint", {
          defaultValue: "请先填写接口地址",
        }),
      );
      return;
    }
    const selectedAccountExists = (
      accountId: string | null,
      accounts: Array<{ id: string }>,
    ) =>
      accountId === null ||
      accounts.some((account) => account.id === accountId);
    const selectedCodexAccountIsUsable = (accountId: string | null) => {
      const effectiveAccountId =
        accountId ??
        codexOauthDefaultAccountId ??
        codexOauthAccounts.find((account) => account.is_default)?.id ??
        codexOauthAccounts[0]?.id;
      return (
        !!effectiveAccountId &&
        codexOauthAccounts.some(
          (account) =>
            account.id === effectiveAccountId && !account.reauth_required,
        )
      );
    };
    const selectedXaiAccountIsUsable = (accountId: string | null) =>
      accountId === null ||
      xaiOauthAccounts.some(
        (account) => account.id === accountId && !account.requires_reauth,
      );
    const managedAuthState =
      activeProviderType === "github_copilot"
        ? {
            authenticated: isCopilotAuthenticated,
            accountId: selectedGitHubAccountId,
            accounts: copilotAccounts,
            loginMessage: t("copilot.loginRequired", {
              defaultValue: "请先登录 GitHub Copilot",
            }),
          }
        : activeProviderType === "codex_oauth"
          ? {
              authenticated: isCodexOauthAuthenticated,
              accountId: selectedCodexAccountId,
              accounts: codexOauthAccounts,
              loginMessage: t("codexOauth.loginRequired", {
                defaultValue: "请先登录 ChatGPT 账号",
              }),
            }
          : activeProviderType === "xai_oauth"
            ? {
                authenticated: isXaiOauthAuthenticated,
                accountId: selectedXaiAccountId,
                accounts: xaiOauthAccounts,
                loginMessage: t("xaiOauth.loginRequired", {
                  defaultValue: "请先登录 xAI 账号",
                }),
              }
            : null;
    if (managedAuthState && !managedAuthState.authenticated) {
      toast.error(managedAuthState.loginMessage);
      return;
    }
    const selectedManagedAccountIsUsable =
      activeProviderType === "codex_oauth"
        ? selectedCodexAccountIsUsable(selectedCodexAccountId)
        : activeProviderType === "xai_oauth"
          ? selectedXaiAccountIsUsable(selectedXaiAccountId)
          : managedAuthState
            ? selectedAccountExists(
                managedAuthState.accountId,
                managedAuthState.accounts,
              )
            : true;
    if (managedAuthState && !selectedManagedAccountIsUsable) {
      toast.error(
        t("managedAuth.selectedAccountUnavailable", {
          defaultValue: "已绑定账号不存在或需要重新登录，请重新选择账号",
        }),
      );
      return;
    }
    if (!usesManagedOAuth && !apiKey.trim()) {
      toast.error(
        t("providerForm.fetchModelsNeedApiKey", {
          defaultValue: "请先填写 API Key",
        }),
      );
      return;
    }

    const routeEntries = routes
      .map((route) => ({
        ...route,
        route: route.route.trim(),
        model: route.model.trim(),
        labelOverride: route.labelOverride.trim(),
      }))
      .filter((route) => route.model);
    if (!routeEntries.length) {
      toast.error(t("claudeDesktop.routesRequired"));
      return;
    }

    const settingsConfig = clonePlainRecord(initialData?.settingsConfig);
    const env = clonePlainRecord(settingsConfig.env);
    delete env.ANTHROPIC_AUTH_TOKEN;
    delete env.ANTHROPIC_API_KEY;
    settingsConfig.env = usesManagedOAuth
      ? {
          ...env,
          ANTHROPIC_BASE_URL: baseUrl.trim().replace(/\/+$/, ""),
        }
      : {
          ...env,
          ANTHROPIC_BASE_URL: baseUrl.trim().replace(/\/+$/, ""),
          [apiKeyField]: apiKey.trim(),
        };

    const routeMap = routeEntries.reduce<
      Record<string, ClaudeDesktopModelRoute>
    >((acc, route) => {
      acc[route.route] = {
        model: route.model || route.route,
        labelOverride: route.labelOverride || route.model,
        supports1m: route.supports1m || undefined,
      };
      return acc;
    }, {});

    const meta: ProviderMeta = {
      ...(initialData?.meta ?? {}),
      apiFormat:
        activeProviderType === "xai_oauth" ? "openai_responses" : apiFormat,
    };

    delete meta.claudeDesktopMode;
    meta.claudeDesktopModelRoutes = routeMap;
    meta.providerType = activeProviderType;
    meta.authBinding =
      activeProviderType === "github_copilot"
        ? {
            source: "managed_account",
            authProvider: "github_copilot",
            accountId: selectedGitHubAccountId ?? undefined,
          }
        : activeProviderType === "codex_oauth"
          ? {
              source: "managed_account",
              authProvider: "codex_oauth",
              accountId: selectedCodexAccountId ?? undefined,
            }
          : activeProviderType === "xai_oauth"
            ? {
                source: "managed_account",
                authProvider: "xai_oauth",
                accountId: selectedXaiAccountId ?? undefined,
              }
            : undefined;
    meta.codexFastMode =
      activeProviderType === "codex_oauth" ? codexFastMode : undefined;

    delete meta.endpointAutoSelect;
    delete meta.isFullUrl;

    await onSubmit({
      ...values,
      name: values.name.trim(),
      websiteUrl: values.websiteUrl?.trim() ?? "",
      notes: values.notes?.trim() ?? "",
      settingsConfig: JSON.stringify(settingsConfig, null, 2),
      meta,
      presetId: activePreset?.id,
      presetCategory: activePreset?.category,
      isPartner: activePreset?.isPartner,
      partnerPromotionKey: activePreset?.partnerPromotionKey,
    });
  };

  const renderActionButtons = (onAdd: () => void, addLabel: string) => (
    <div className="flex flex-wrap justify-end gap-1">
      {!usesManagedOAuth && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void handleFetchModels()}
          disabled={isFetchingModels}
          className="h-7 gap-1"
        >
          {isFetchingModels ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Download className="h-3.5 w-3.5" />
          )}
          {t("providerForm.fetchModels", { defaultValue: "获取模型" })}
        </Button>
      )}
      {!usesManagedOAuth && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void handleFetchModels(true)}
          disabled={isFetchingModels}
          className="h-7 gap-1"
        >
          <Download className="h-3.5 w-3.5" />
          {t("claudeDesktop.importAllModels", {
            defaultValue: "一键导入所有模型",
          })}
        </Button>
      )}
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={onAdd}
        className="h-7 gap-1"
      >
        <Plus className="h-3.5 w-3.5" />
        {addLabel}
      </Button>
    </div>
  );

  return (
    <Form {...form}>
      <form
        id="provider-form"
        onSubmit={form.handleSubmit(handleSubmit)}
        onFocusCapture={(event) => {
          if (["baseUrl", "apiKey"].includes(event.target.id))
            setEditingCredentials(true);
        }}
        onBlurCapture={(event) => {
          if (["baseUrl", "apiKey"].includes(event.target.id))
            setEditingCredentials(false);
        }}
        className="space-y-6 glass rounded-xl p-6 border border-white/10"
      >
        {!initialData && (
          <ProviderPresetSelector
            selectedPresetId={selectedPresetId}
            presetEntries={presetEntries}
            presetCategoryLabels={presetCategoryLabels}
            onPresetChange={handlePresetChange}
            category={activePreset?.category}
          />
        )}

        <ConnectionPresetPicker
          disabled={isOfficial || usesManagedOAuth}
          current={{
            name: form.watch("name"),
            baseUrl,
            apiKey,
            apiFormat,
          }}
          onApply={(preset) => {
            if (preset.apiFormat) setApiFormat(preset.apiFormat);
            form.setValue("name", preset.name);
            setBaseUrl(preset.baseUrl);
            setApiKey(preset.apiKey);
          }}
        />
        <BasicFormFields form={form} />

        {isOfficial && (
          <div className="rounded-lg border border-border-default bg-muted/20 p-3 text-sm text-muted-foreground">
            {t("claudeDesktop.officialNotice", {
              defaultValue:
                "Claude Desktop 官方供应商使用应用内置的 1P 登录，无需配置 API Key 和接口地址。",
            })}
          </div>
        )}

        {!isOfficial && (
          <>
            {usesManagedOAuth ? (
              <div className="rounded-lg border border-border-default bg-muted/20 p-3">
                {activeProviderType === "github_copilot" ? (
                  <CopilotAuthSection
                    mode="select"
                    selectedAccountId={selectedGitHubAccountId}
                    onAccountSelect={setSelectedGitHubAccountId}
                    onManageAccounts={
                      onManageAuthAccounts
                        ? () => onManageAuthAccounts("github_copilot")
                        : undefined
                    }
                  />
                ) : activeProviderType === "codex_oauth" ? (
                  <CodexOAuthSection
                    mode="select"
                    selectedAccountId={selectedCodexAccountId}
                    onAccountSelect={setSelectedCodexAccountId}
                    onManageAccounts={
                      onManageAuthAccounts
                        ? () => onManageAuthAccounts("codex_oauth")
                        : undefined
                    }
                    fastModeEnabled={codexFastMode}
                    onFastModeChange={setCodexFastMode}
                  />
                ) : (
                  <XaiOAuthSection
                    selectedAccountId={selectedXaiAccountId}
                    onAccountSelect={setSelectedXaiAccountId}
                  />
                )}
              </div>
            ) : (
              <ApiKeySection
                value={apiKey}
                onChange={setApiKey}
                category={apiKeyLinkCategory}
                shouldShowLink={shouldShowApiKeyLink}
                websiteUrl={apiKeyLinkWebsiteUrl}
                isPartner={apiKeyLinkIsPartner}
                partnerPromotionKey={apiKeyLinkPromotionKey}
              />
            )}

            <EndpointField
              id="baseUrl"
              label={t("providerForm.apiEndpoint")}
              value={baseUrl}
              onChange={(v) => setBaseUrl(v)}
              placeholder={t("providerForm.apiEndpointPlaceholder")}
              hint={
                apiFormat === "openai_responses"
                  ? t("providerForm.apiHintResponses")
                  : apiFormat === "openai_chat"
                    ? t("providerForm.apiHintOAI")
                    : apiFormat === "gemini_native"
                      ? t("providerForm.apiHintGeminiNative")
                      : t("providerForm.apiHint")
              }
              showManageButton={false}
            />

            <div className="space-y-4 border-l border-border-default pl-3">
              <div className="flex items-stretch justify-between gap-4">
                <div className="min-w-0 flex-1 space-y-1 pr-3">
                  <Label>
                    {t("claudeDesktop.modelConfigTitle", {
                      defaultValue: "模型配置",
                    })}
                  </Label>
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    {t("claudeDesktop.modelRoutingHint")}
                  </p>
                </div>
              </div>

              {!usesManagedOAuth && (
                <ProviderConfigDetection
                  baseUrl={baseUrl}
                  apiKey={apiKey}
                  model={routes.find((row) => row.model.trim())?.model || ""}
                  apiFormat={apiFormat}
                  anthropicAuthField={apiKeyField}
                  editingCredentials={editingCredentials}
                  autoSelectFormat={!initialData && !protocolManuallySelected}
                  onManualFormatSelect={() => setProtocolManuallySelected(true)}
                  onApiFormatChange={(format) => {
                    setApiFormat(format);
                  }}
                  catalogModels={fetchedModels}
                  onCatalogModelsChange={setFetchedModels}
                />
              )}

              <div className="space-y-4 border-t border-border-default pt-4">
                {activeProviderType !== "xai_oauth" && (
                  <div className="space-y-2">
                    <Label>
                      {t("providerForm.apiFormat", {
                        defaultValue: "上游格式",
                      })}
                    </Label>
                    <Select
                      value={apiFormat}
                      onValueChange={(value) => {
                        setProtocolManuallySelected(true);
                        setApiFormat(value as ClaudeApiFormat);
                      }}
                    >
                      <SelectTrigger className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="anthropic">
                          {t("providerForm.apiFormatAnthropic", {
                            defaultValue: "Anthropic Messages (原生)",
                          })}
                        </SelectItem>
                        <SelectItem value="openai_chat">
                          {t("providerForm.apiFormatOpenAIChat", {
                            defaultValue:
                              "OpenAI Chat Completions (需开启路由)",
                          })}
                        </SelectItem>
                        <SelectItem value="openai_responses">
                          {t("providerForm.apiFormatOpenAIResponses", {
                            defaultValue: "OpenAI Responses API (需开启路由)",
                          })}
                        </SelectItem>
                        <SelectItem value="gemini_native">
                          {t("providerForm.apiFormatGeminiNative", {
                            defaultValue:
                              "Gemini Native generateContent (需开启路由)",
                          })}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                )}

                <div className="space-y-3">
                  <div className="space-y-1 border-t border-border-default pt-4">
                    <div className="flex items-center justify-between">
                      <Label>
                        {t("claudeDesktop.routeMapTitle", {
                          defaultValue: "模型映射",
                        })}
                      </Label>
                      {renderActionButtons(
                        addProxyRoute,
                        t("claudeDesktop.addModel", {
                          defaultValue: "添加模型",
                        }),
                      )}
                    </div>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("claudeDesktop.routeMapHint", {
                        defaultValue:
                          "Add multiple models per role. Menu display names are customizable and update when selecting a model. Only rows with an actual request model appear in Desktop's menu. Declare 1M only when supported by the upstream.",
                      })}
                    </p>
                  </div>

                  <div className="hidden grid-cols-[140px_1fr_1fr_116px_36px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
                    <span>
                      {t("claudeDesktop.routeModelLabel", {
                        defaultValue: "模型角色",
                      })}
                    </span>
                    <span>
                      {t("claudeDesktop.labelOverrideLabel", {
                        defaultValue: "菜单显示名",
                      })}
                    </span>
                    <span>
                      {t("claudeDesktop.upstreamModelLabel", {
                        defaultValue: "实际请求模型",
                      })}
                    </span>
                    <span>
                      {t("claudeDesktop.supports1mLabel", {
                        defaultValue: "声明支持 1M",
                      })}
                    </span>
                  </div>
                  {routes.map((route, index) => {
                    const role = routeRoleFromId(route.route);
                    // Use lightweight placeholders for Haiku and stronger models for other roles.
                    // Keep display-name and request-model placeholders consistent.
                    const isHaikuRole = role === "haiku";
                    const labelPlaceholder = isHaikuRole
                      ? "DeepSeek V4 Flash"
                      : "DeepSeek V4 Pro";
                    const modelPlaceholder = isHaikuRole
                      ? "deepseek-v4-flash"
                      : "deepseek-v4-pro";
                    return (
                      <div
                        key={route.rowId}
                        className="grid grid-cols-1 gap-2 md:grid-cols-[140px_1fr_1fr_116px_36px]"
                      >
                        <Select
                          value={role}
                          onValueChange={(value) =>
                            updateRoute(index, {
                              route: routeIdForRole(
                                value as RouteRole,
                                new Set(
                                  routes
                                    .filter((_, i) => i !== index)
                                    .map((row) => row.route),
                                ),
                              ),
                            })
                          }
                        >
                          <SelectTrigger
                            aria-label={t("claudeDesktop.routeModelLabel", {
                              defaultValue: "模型角色",
                            })}
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {ROLE_ORDER.map((candidate) => {
                              const label =
                                candidate[0].toUpperCase() + candidate.slice(1);
                              return (
                                <SelectItem key={candidate} value={candidate}>
                                  {t(`claudeDesktop.routeRole${label}`, {
                                    defaultValue: label,
                                  })}
                                </SelectItem>
                              );
                            })}
                          </SelectContent>
                        </Select>
                        <Input
                          aria-label={t("claudeDesktop.labelOverrideLabel", {
                            defaultValue: "菜单显示名",
                          })}
                          value={route.labelOverride}
                          onChange={(event) =>
                            updateRoute(index, {
                              labelOverride: event.target.value,
                            })
                          }
                          placeholder={labelPlaceholder}
                        />
                        <div className="flex gap-1">
                          <Input
                            aria-label={t("claudeDesktop.upstreamModelLabel", {
                              defaultValue: "实际请求模型",
                            })}
                            value={route.model}
                            onChange={(event) =>
                              updateRoute(index, {
                                model: event.target.value,
                              })
                            }
                            placeholder={modelPlaceholder}
                            className="flex-1"
                          />
                          {fetchedModels.length > 0 && (
                            <ModelDropdown
                              models={fetchedModels}
                              onSelect={(id) =>
                                updateRoute(index, {
                                  model: id,
                                  labelOverride: id,
                                })
                              }
                            />
                          )}
                        </div>
                        <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                          <Checkbox
                            checked={route.supports1m}
                            onCheckedChange={(checked) =>
                              updateRoute(index, {
                                supports1m: checked === true,
                              })
                            }
                          />
                          {t("claudeDesktop.supports1mShort", {
                            defaultValue: "1M",
                          })}
                        </label>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          aria-label={t("common.delete", {
                            defaultValue: "Delete",
                          })}
                          onClick={() => {
                            didSeedDefaultProxyRoutes.current = true;
                            setProxyRoutes((current) =>
                              current.filter((_, i) => i !== index),
                            );
                          }}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>

            <FormField
              control={form.control}
              name="settingsConfig"
              render={() => (
                <FormItem className="space-y-0">
                  <FormControl>
                    <input type="hidden" />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
          </>
        )}

        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button variant="outline" type="button" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={form.formState.isSubmitting}>
              {submitLabel}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
