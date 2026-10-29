import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "sonner";
import { Checkbox } from "@/components/ui/checkbox";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Wand2,
  Trash2,
} from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import {
  ApiKeySection,
  EndpointField,
  ModelDropdown,
  ModelInputWithFetch,
} from "./shared";
import { CopilotAuthSection } from "./CopilotAuthSection";
import { CodexOAuthSection } from "./CodexOAuthSection";
import { XaiOAuthSection } from "./XaiOAuthSection";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
} from "@/lib/api/copilot";
import type { CopilotModel } from "@/lib/api/copilot";
import {
  fetchCodexOauthModels,
  fetchXaiOauthModels,
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { CustomUserAgentField } from "./CustomUserAgentField";
import { LocalProxyRequestOverridesField } from "./LocalProxyRequestOverridesField";
import { ProviderConfigDetection } from "./ProviderConfigDetection";
import type {
  ProviderCategory,
  ClaudeApiFormat,
  ClaudeApiKeyField,
} from "@/types";
import type { ManagedAuthProvider } from "@/lib/api";
import {
  hasClaudeOneMMarker,
  setClaudeOneMMarker,
  stripClaudeOneMMarker,
  type ClaudeModelEnvField,
  type ClaudeModelPickerOption,
} from "./hooks/useModelState";
import {
  providerPresets,
  type TemplateValueConfig,
} from "@/config/claudeProviderPresets";

interface EndpointCandidate {
  url: string;
}

interface ClaudeFormFieldsProps {
  providerId?: string;
  // API Key
  shouldShowApiKey: boolean;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // GitHub Copilot OAuth
  isCopilotPreset?: boolean;
  usesOAuth?: boolean;
  isCopilotAuthenticated?: boolean;
  /** 当前选中的 GitHub 账号 ID（多账号支持） */
  selectedGitHubAccountId?: string | null;
  /** GitHub 账号选择回调（多账号支持） */
  onGitHubAccountSelect?: (accountId: string | null) => void;
  /** 打开托管账号管理入口 */
  onManageAuthAccounts?: (target: ManagedAuthProvider) => void;

  // Codex OAuth (ChatGPT Plus/Pro)
  isCodexOauthPreset?: boolean;
  isCodexOauthAuthenticated?: boolean;
  selectedCodexAccountId?: string | null;
  onCodexAccountSelect?: (accountId: string | null) => void;
  codexFastMode?: boolean;
  onCodexFastModeChange?: (enabled: boolean) => void;

  // xAI OAuth
  isXaiOauthPreset?: boolean;
  isXaiOauthAuthenticated?: boolean;
  selectedXaiAccountId?: string | null;
  onXaiAccountSelect?: (accountId: string | null) => void;

  // Template Values
  templateValueEntries: Array<[string, TemplateValueConfig]>;
  templateValues: Record<string, TemplateValueConfig>;
  templatePresetName: string;
  onTemplateValueChange: (key: string, value: string) => void;

  // Base URL
  shouldShowSpeedTest: boolean;
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange?: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;
  showEndpointTools?: boolean;

  // Model Selector
  shouldShowModelSelector: boolean;
  claudeModel: string;
  defaultHaikuModel: string;
  defaultHaikuModelName: string;
  defaultSonnetModel: string;
  defaultSonnetModelName: string;
  defaultOpusModel: string;
  defaultOpusModelName: string;
  defaultFableModel: string;
  defaultFableModelName: string;
  subagentModel: string;
  onModelChange: (field: ClaudeModelEnvField, value: string) => void;
  modelPickerOptions: ClaudeModelPickerOption[];
  onModelPickerOptionsChange: (options: ClaudeModelPickerOption[]) => void;
  configurationDetection?: {
    editingCredentials: boolean;
    autoSelectFormat: boolean;
    onManualFormatSelect?: () => void;
    onApiFormatChange?: (format: ClaudeApiFormat) => void;
  };

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];

  // API Format (for Claude-compatible providers that need request/response conversion)
  apiFormat: ClaudeApiFormat;
  onApiFormatChange: (format: ClaudeApiFormat) => void;

  // Auth Field (ANTHROPIC_AUTH_TOKEN or ANTHROPIC_API_KEY)
  apiKeyField: ClaudeApiKeyField;
  onApiKeyFieldChange: (field: ClaudeApiKeyField) => void;

  // Full URL mode
  isFullUrl: boolean;
  onFullUrlChange: (value: boolean) => void;

  // Local proxy User-Agent override
  customUserAgent: string;
  onCustomUserAgentChange: (value: string) => void;
  localProxyHeadersOverride: string;
  onLocalProxyHeadersOverrideChange: (value: string) => void;
  localProxyBodyOverride: string;
  onLocalProxyBodyOverrideChange: (value: string) => void;
}

export function ClaudeFormFields({
  providerId,
  shouldShowApiKey,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  isCopilotPreset,
  usesOAuth,
  isCopilotAuthenticated,
  selectedGitHubAccountId,
  onGitHubAccountSelect,
  onManageAuthAccounts,
  isCodexOauthPreset,
  isCodexOauthAuthenticated,
  selectedCodexAccountId,
  onCodexAccountSelect,
  codexFastMode,
  onCodexFastModeChange,
  isXaiOauthPreset,
  isXaiOauthAuthenticated,
  selectedXaiAccountId,
  onXaiAccountSelect,
  templateValueEntries,
  templateValues,
  templatePresetName,
  onTemplateValueChange,
  shouldShowSpeedTest,
  baseUrl,
  onBaseUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  showEndpointTools = true,
  shouldShowModelSelector,
  claudeModel,
  defaultHaikuModel,
  defaultHaikuModelName,
  defaultSonnetModel,
  defaultSonnetModelName,
  defaultOpusModel,
  defaultOpusModelName,
  defaultFableModel,
  defaultFableModelName,
  subagentModel,
  onModelChange,
  modelPickerOptions,
  onModelPickerOptionsChange,
  configurationDetection,
  speedTestEndpoints,
  apiFormat,
  onApiFormatChange,
  apiKeyField,
  onApiKeyFieldChange,
  isFullUrl,
  onFullUrlChange,
  customUserAgent,
  onCustomUserAgentChange,
  localProxyHeadersOverride,
  onLocalProxyHeadersOverrideChange,
  localProxyBodyOverride,
  onLocalProxyBodyOverrideChange,
}: ClaudeFormFieldsProps) {
  const { t } = useTranslation();
  const hasRequestOverrides = Boolean(
    localProxyHeadersOverride.trim() || localProxyBodyOverride.trim(),
  );
  const hasAnyAdvancedValue = !!(
    modelPickerOptions.length ||
    claudeModel ||
    defaultHaikuModel ||
    defaultSonnetModel ||
    defaultOpusModel ||
    defaultFableModel ||
    subagentModel ||
    (!isXaiOauthPreset && apiFormat !== "anthropic") ||
    apiKeyField !== "ANTHROPIC_AUTH_TOKEN" ||
    customUserAgent ||
    hasRequestOverrides
  );
  const [advancedExpanded, setAdvancedExpanded] = useState(
    isXaiOauthPreset ? false : hasAnyAdvancedValue,
  );

  // 预设填充高级值后自动展开（仅从折叠→展开，不会自动折叠）
  useEffect(() => {
    if (isXaiOauthPreset) {
      setAdvancedExpanded(false);
    } else if (hasAnyAdvancedValue) {
      setAdvancedExpanded(true);
    }
  }, [hasAnyAdvancedValue, isXaiOauthPreset]);

  // Copilot 可用模型列表
  const [copilotModels, setCopilotModels] = useState<CopilotModel[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const copilotModelsRequestRef = useRef(0);

  // Codex OAuth 可用模型列表
  const [codexOauthModels, setCodexOauthModels] = useState<FetchedModel[]>([]);
  const [codexOauthModelsLoading, setCodexOauthModelsLoading] = useState(false);
  const codexOauthModelsRequestRef = useRef(0);

  const [xaiOauthModels, setXaiOauthModels] = useState<FetchedModel[]>([]);
  const [xaiOauthModelsLoading, setXaiOauthModelsLoading] = useState(false);
  const xaiOauthModelsRequestRef = useRef(0);
  const fallbackUsesOneM = hasClaudeOneMMarker(claudeModel);

  // 通用模型获取（非 Copilot 供应商）
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const modelCatalogRequest = useRef(0);
  const [importingModels, setImportingModels] = useState(false);
  const latestModelPickerOptions = useRef(modelPickerOptions);
  latestModelPickerOptions.current = modelPickerOptions;

  useEffect(() => {
    modelCatalogRequest.current += 1;
    setFetchedModels([]);
    setIsFetchingModels(false);
    return () => {
      modelCatalogRequest.current += 1;
    };
  }, [baseUrl, apiKey, isFullUrl, customUserAgent, apiFormat]);

  const showModelFetchResult = useCallback(
    (count: number) => {
      if (count === 0) {
        toast.info(t("providerForm.fetchModelsEmpty"));
      } else {
        toast.success(t("providerForm.fetchModelsSuccess", { count }));
      }
    },
    [t],
  );

  const handleFetchModels = useCallback(
    (importModels = false) => {
      if (!baseUrl || !apiKey) {
        showFetchModelsError(null, t, {
          hasApiKey: !!apiKey,
          hasBaseUrl: !!baseUrl,
        });
        return;
      }
      // 当 baseURL 仍是某预设的默认值时，优先使用预设上的 modelsUrl 覆写
      // 避免多走一次失败的候选请求（如 DeepSeek 把 /models 挂在根，而不是 /anthropic 子路径下）
      const matchedPreset = providerPresets.find((p) => {
        const env = (p.settingsConfig as { env?: Record<string, string> })?.env;
        return env?.ANTHROPIC_BASE_URL === baseUrl;
      });
      const modelsUrl = matchedPreset?.modelsUrl;

      const request = ++modelCatalogRequest.current;
      setIsFetchingModels(true);
      return fetchModelsForConfig(
        baseUrl,
        apiKey,
        isFullUrl,
        modelsUrl,
        customUserAgent,
        {
          apiFormat:
            apiFormat === "gemini_native" ? "google-generative-ai" : undefined,
        },
      )
        .then((models) => {
          if (request !== modelCatalogRequest.current) return;
          setFetchedModels(models);
          if (!importModels) showModelFetchResult(models.length);
          return models;
        })
        .catch((err) => {
          if (request !== modelCatalogRequest.current) return;
          showFetchModelsError(err, t);
        })
        .finally(() => {
          if (request === modelCatalogRequest.current)
            setIsFetchingModels(false);
        });
    },
    [
      baseUrl,
      apiKey,
      isFullUrl,
      customUserAgent,
      apiFormat,
      showModelFetchResult,
      t,
    ],
  );

  const handleFetchCopilotModels = useCallback(
    (importModels = false) => {
      if (!isCopilotAuthenticated) {
        toast.error(
          t("copilot.loginRequired", {
            defaultValue: "请先登录 GitHub Copilot",
          }),
        );
        return;
      }

      const requestId = copilotModelsRequestRef.current + 1;
      copilotModelsRequestRef.current = requestId;
      setModelsLoading(true);
      const fetchModels = selectedGitHubAccountId
        ? copilotGetModelsForAccount(selectedGitHubAccountId)
        : copilotGetModels();

      return fetchModels
        .then((models) => {
          if (copilotModelsRequestRef.current !== requestId) return;
          setCopilotModels(models);
          if (!importModels) showModelFetchResult(models.length);
          return models;
        })
        .catch((err) => {
          if (copilotModelsRequestRef.current !== requestId) return;
          console.warn("[Copilot] Failed to fetch models:", err);
          toast.error(
            t("copilot.loadModelsFailed", {
              defaultValue: "加载 Copilot 模型列表失败",
            }),
          );
        })
        .finally(() => {
          if (copilotModelsRequestRef.current === requestId) {
            setModelsLoading(false);
          }
        });
    },
    [isCopilotAuthenticated, selectedGitHubAccountId, showModelFetchResult, t],
  );

  const handleFetchCodexOauthModels = useCallback(
    (importModels = false) => {
      if (!isCodexOauthAuthenticated) {
        toast.error(
          t("codexOauth.loginRequired", {
            defaultValue: "请先登录 ChatGPT 账号",
          }),
        );
        return;
      }

      const requestId = codexOauthModelsRequestRef.current + 1;
      codexOauthModelsRequestRef.current = requestId;
      setCodexOauthModelsLoading(true);
      return fetchCodexOauthModels(selectedCodexAccountId)
        .then((models) => {
          if (codexOauthModelsRequestRef.current !== requestId) return;
          setCodexOauthModels(models);
          if (!importModels) showModelFetchResult(models.length);
          return models;
        })
        .catch((err) => {
          if (codexOauthModelsRequestRef.current !== requestId) return;
          console.warn("[CodexOAuth] Failed to fetch models:", err);
          showFetchModelsError(err, t);
        })
        .finally(() => {
          if (codexOauthModelsRequestRef.current === requestId) {
            setCodexOauthModelsLoading(false);
          }
        });
    },
    [
      isCodexOauthAuthenticated,
      selectedCodexAccountId,
      showModelFetchResult,
      t,
    ],
  );

  const handleFetchXaiOauthModels = useCallback(
    (importModels = false) => {
      if (!isXaiOauthAuthenticated) {
        toast.error(
          t("xaiOauth.loginRequired", {
            defaultValue: "请先登录 xAI 账号",
          }),
        );
        return;
      }

      const requestId = xaiOauthModelsRequestRef.current + 1;
      xaiOauthModelsRequestRef.current = requestId;
      setXaiOauthModelsLoading(true);
      return fetchXaiOauthModels(selectedXaiAccountId)
        .then((models) => {
          if (xaiOauthModelsRequestRef.current !== requestId) return;
          setXaiOauthModels(models);
          if (!importModels) showModelFetchResult(models.length);
          return models;
        })
        .catch((err) => {
          if (xaiOauthModelsRequestRef.current !== requestId) return;
          console.warn("[XaiOAuth] Failed to fetch models:", err);
          showFetchModelsError(err, t);
        })
        .finally(() => {
          if (xaiOauthModelsRequestRef.current === requestId) {
            setXaiOauthModelsLoading(false);
          }
        });
    },
    [isXaiOauthAuthenticated, selectedXaiAccountId, showModelFetchResult, t],
  );

  useEffect(() => {
    copilotModelsRequestRef.current += 1;
    setCopilotModels([]);
    setModelsLoading(false);
  }, [isCopilotPreset, isCopilotAuthenticated, selectedGitHubAccountId]);

  useEffect(() => {
    codexOauthModelsRequestRef.current += 1;
    setCodexOauthModels([]);
    setCodexOauthModelsLoading(false);
  }, [isCodexOauthPreset, isCodexOauthAuthenticated, selectedCodexAccountId]);

  useEffect(() => {
    xaiOauthModelsRequestRef.current += 1;
    setXaiOauthModels([]);
    setXaiOauthModelsLoading(false);
  }, [isXaiOauthPreset, isXaiOauthAuthenticated, selectedXaiAccountId]);

  const modelFetchLoading = isCopilotPreset
    ? modelsLoading
    : isCodexOauthPreset
      ? codexOauthModelsLoading
      : isXaiOauthPreset
        ? xaiOauthModelsLoading
        : isFetchingModels;
  const handleModelFetchClick = isCopilotPreset
    ? handleFetchCopilotModels
    : isCodexOauthPreset
      ? handleFetchCodexOauthModels
      : isXaiOauthPreset
        ? handleFetchXaiOauthModels
        : handleFetchModels;

  // 模型输入框：支持手动输入 + 下拉选择
  const renderModelInput = (
    id: string,
    value: string,
    field: ClaudeModelEnvField,
    placeholder?: string,
    onValueChange?: (value: string) => void,
  ) => {
    const updateValue =
      onValueChange ?? ((next: string) => onModelChange(field, next));

    if (isCodexOauthPreset) {
      return (
        <ModelInputWithFetch
          id={id}
          value={value}
          onChange={updateValue}
          placeholder={placeholder}
          fetchedModels={codexOauthModels}
          isLoading={codexOauthModelsLoading}
        />
      );
    }

    if (isXaiOauthPreset) {
      return (
        <ModelInputWithFetch
          id={id}
          value={value}
          onChange={updateValue}
          placeholder={placeholder}
          fetchedModels={xaiOauthModels}
          isLoading={xaiOauthModelsLoading}
        />
      );
    }

    if (isCopilotPreset && copilotModels.length > 0) {
      // Reuse the searchable dropdown by mapping Copilot models to FetchedModel.
      const copilotFetchedModels: FetchedModel[] = copilotModels.map((m) => ({
        id: m.id,
        ownedBy: m.vendor || null,
      }));

      return (
        <div className="flex gap-1">
          <Input
            id={id}
            type="text"
            value={value}
            onChange={(e) => updateValue(e.target.value)}
            placeholder={placeholder}
            autoComplete="off"
            className="flex-1"
          />
          <ModelDropdown models={copilotFetchedModels} onSelect={updateValue} />
        </div>
      );
    }

    if (isCopilotPreset && modelsLoading) {
      return (
        <div className="flex gap-1">
          <Input
            id={id}
            type="text"
            value={value}
            onChange={(e) => updateValue(e.target.value)}
            placeholder={placeholder}
            autoComplete="off"
            className="flex-1"
          />
          <Button variant="outline" size="icon" className="shrink-0" disabled>
            <Loader2 className="h-4 w-4 animate-spin" />
          </Button>
        </div>
      );
    }

    if (isCopilotPreset) {
      return (
        <Input
          id={id}
          type="text"
          value={value}
          onChange={(e) => updateValue(e.target.value)}
          placeholder={placeholder}
          autoComplete="off"
        />
      );
    }

    // 普通供应商: 使用 ModelInputWithFetch（获取按钮在 section 标题旁）
    return (
      <ModelInputWithFetch
        id={id}
        value={value}
        onChange={updateValue}
        placeholder={placeholder}
        fetchedModels={fetchedModels}
        isLoading={isFetchingModels}
      />
    );
  };

  type ModelRoleRow = {
    role: "sonnet" | "opus" | "fable" | "haiku" | "subagent";
    label: string;
    model: string;
    displayName?: string;
    modelField: ClaudeModelEnvField;
    displayNameField?: ClaudeModelEnvField;
    inputId: string;
    supportsOneM: boolean;
  };

  const modelRoleRows: ModelRoleRow[] = [
    {
      role: "sonnet",
      label: t("providerForm.modelRoleSonnet", { defaultValue: "Sonnet" }),
      model: defaultSonnetModel,
      displayName: defaultSonnetModelName,
      modelField: "ANTHROPIC_DEFAULT_SONNET_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
      inputId: "claudeDefaultSonnetModel",
      supportsOneM: true,
    },
    {
      role: "opus",
      label: t("providerForm.modelRoleOpus", { defaultValue: "Opus" }),
      model: defaultOpusModel,
      displayName: defaultOpusModelName,
      modelField: "ANTHROPIC_DEFAULT_OPUS_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
      inputId: "claudeDefaultOpusModel",
      supportsOneM: true,
    },
    {
      role: "fable",
      label: t("providerForm.modelRoleFable", { defaultValue: "Fable" }),
      model: defaultFableModel,
      displayName: defaultFableModelName,
      modelField: "ANTHROPIC_DEFAULT_FABLE_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
      inputId: "claudeDefaultFableModel",
      supportsOneM: true,
    },
    {
      role: "haiku",
      label: t("providerForm.modelRoleHaiku", { defaultValue: "Haiku" }),
      model: defaultHaikuModel,
      displayName: defaultHaikuModelName,
      modelField: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
      displayNameField: "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
      inputId: "claudeDefaultHaikuModel",
      supportsOneM: false,
    },
    {
      role: "subagent",
      label: t("providerForm.modelRoleSubagent", {
        defaultValue: "Subagent",
      }),
      model: subagentModel,
      modelField: "CLAUDE_CODE_SUBAGENT_MODEL",
      inputId: "claudeCodeSubagentModel",
      supportsOneM: true,
    },
  ];

  const handleRoleModelChange = (row: ModelRoleRow, value: string) => {
    const oldModelBase = stripClaudeOneMMarker(row.model).trim();
    const normalizedValue = row.supportsOneM
      ? value
      : stripClaudeOneMMarker(value);
    const nextModelBase = stripClaudeOneMMarker(normalizedValue).trim();
    const displayName = row.displayName?.trim() ?? "";
    const shouldSyncDisplayName = !displayName || displayName === oldModelBase;
    onModelChange(row.modelField, normalizedValue);
    if (row.displayNameField && shouldSyncDisplayName) {
      onModelChange(row.displayNameField, nextModelBase);
    }
  };

  const handleRoleOneMChange = (row: ModelRoleRow, enabled: boolean) => {
    if (!row.supportsOneM) return;
    handleRoleModelChange(row, setClaudeOneMMarker(row.model, enabled));
  };

  const detectionModel = stripClaudeOneMMarker(
    claudeModel ||
      defaultSonnetModel ||
      defaultOpusModel ||
      defaultFableModel ||
      defaultHaikuModel ||
      subagentModel,
  );

  const availableModels = isCopilotPreset
    ? copilotModels
    : isCodexOauthPreset
      ? codexOauthModels
      : isXaiOauthPreset
        ? xaiOauthModels
        : fetchedModels;
  const canFetchModels = isCopilotPreset
    ? isCopilotAuthenticated
    : isCodexOauthPreset
      ? isCodexOauthAuthenticated
      : isXaiOauthPreset
        ? isXaiOauthAuthenticated
        : Boolean(baseUrl.trim() && apiKey.trim());

  const handleQuickSet = async () => {
    setImportingModels(true);
    try {
      const models = availableModels.length
        ? availableModels
        : await handleModelFetchClick(true);
      if (!models) return;
      const options = [...latestModelPickerOptions.current];
      const known = new Set(options.map((option) => option.model.trim()));
      for (const { id } of models) {
        const model = id.trim();
        if (!model || known.has(model)) continue;
        known.add(model);
        options.push({ model, label: model });
      }
      if (!models.length) {
        toast.info(t("providerForm.fetchModelsEmpty"));
        return;
      }
      onModelPickerOptionsChange(options);
      setAdvancedExpanded(true);
      toast.success(
        t("providerForm.quickSetSuccess", { count: options.length }),
      );
    } finally {
      setImportingModels(false);
    }
  };

  return (
    <>
      {/* GitHub Copilot OAuth 认证 */}
      {isCopilotPreset && (
        <CopilotAuthSection
          mode="select"
          selectedAccountId={selectedGitHubAccountId}
          onAccountSelect={onGitHubAccountSelect}
          onManageAccounts={
            onManageAuthAccounts
              ? () => onManageAuthAccounts("github_copilot")
              : undefined
          }
        />
      )}

      {/* Codex OAuth 认证 (ChatGPT Plus/Pro) */}
      {isCodexOauthPreset && (
        <CodexOAuthSection
          mode="select"
          selectedAccountId={selectedCodexAccountId}
          onAccountSelect={onCodexAccountSelect}
          onManageAccounts={
            onManageAuthAccounts
              ? () => onManageAuthAccounts("codex_oauth")
              : undefined
          }
          fastModeEnabled={codexFastMode}
          onFastModeChange={onCodexFastModeChange}
        />
      )}

      {isXaiOauthPreset && (
        <XaiOAuthSection
          selectedAccountId={selectedXaiAccountId}
          onAccountSelect={onXaiAccountSelect}
        />
      )}

      {/* API Key 输入框（非 OAuth 预设时显示） */}
      {shouldShowApiKey && !usesOAuth && (
        <ApiKeySection
          value={apiKey}
          onChange={onApiKeyChange}
          category={category}
          shouldShowLink={shouldShowApiKeyLink}
          websiteUrl={websiteUrl}
          isPartner={isPartner}
          partnerPromotionKey={partnerPromotionKey}
        />
      )}

      {/* 模板变量输入 */}
      {templateValueEntries.length > 0 && (
        <div className="space-y-3">
          <FormLabel>
            {t("providerForm.parameterConfig", {
              name: templatePresetName,
              defaultValue: `${templatePresetName} 参数配置`,
            })}
          </FormLabel>
          <div className="space-y-4">
            {templateValueEntries.map(([key, config]) => (
              <div key={key} className="space-y-2">
                <FormLabel htmlFor={`template-${key}`}>
                  {config.label}
                </FormLabel>
                <Input
                  id={`template-${key}`}
                  type="text"
                  required
                  value={
                    templateValues[key]?.editorValue ??
                    config.editorValue ??
                    config.defaultValue ??
                    ""
                  }
                  onChange={(e) => onTemplateValueChange(key, e.target.value)}
                  placeholder={config.placeholder || config.label}
                  autoComplete="off"
                />
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Base URL 输入框 */}
      {shouldShowSpeedTest && (
        <EndpointField
          id="baseUrl"
          label={t("providerForm.apiEndpoint")}
          value={baseUrl}
          onChange={onBaseUrlChange}
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
          fullUrlHint={
            apiFormat === "gemini_native"
              ? t("providerForm.fullUrlHintGeminiNative")
              : undefined
          }
          showManageButton={showEndpointTools}
          onManageClick={
            showEndpointTools ? () => onEndpointModalToggle(true) : undefined
          }
          showFullUrlToggle={showEndpointTools && !isXaiOauthPreset}
          isFullUrl={isFullUrl}
          onFullUrlChange={onFullUrlChange}
        />
      )}

      {/* 端点测速弹窗 */}
      {shouldShowSpeedTest && showEndpointTools && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId="claude"
          providerId={providerId}
          value={baseUrl}
          onChange={onBaseUrlChange}
          initialEndpoints={speedTestEndpoints}
          visible={isEndpointModalOpen}
          onClose={() => onEndpointModalToggle(false)}
          autoSelect={autoSelect}
          onAutoSelectChange={onAutoSelectChange}
          onCustomEndpointsChange={onCustomEndpointsChange}
        />
      )}

      {shouldShowModelSelector && (
        <Collapsible
          open={advancedExpanded}
          onOpenChange={setAdvancedExpanded}
          className="rounded-lg border border-border-default p-4"
        >
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant={null}
              size="sm"
              className="h-8 w-full justify-start gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
            >
              {advancedExpanded ? (
                <ChevronDown className="h-4 w-4" />
              ) : (
                <ChevronRight className="h-4 w-4" />
              )}
              {t("providerForm.advancedOptionsToggle")}
            </Button>
          </CollapsibleTrigger>
          {!advancedExpanded && (
            <p className="text-xs text-muted-foreground mt-1 ml-1">
              {t("providerForm.advancedOptionsHint")}
            </p>
          )}
          <CollapsibleContent className="space-y-4 pt-2">
            {/* 上游格式选择（仅非云服务商显示） */}
            {category !== "cloud_provider" && !isXaiOauthPreset && (
              <div className="space-y-2">
                <FormLabel htmlFor="apiFormat">
                  {t("providerForm.apiFormat", { defaultValue: "上游格式" })}
                </FormLabel>
                <Select value={apiFormat} onValueChange={onApiFormatChange}>
                  <SelectTrigger id="apiFormat" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="anthropic">
                      {t("providerForm.apiFormatAnthropic", {
                        defaultValue: "Anthropic Messages",
                      })}
                    </SelectItem>
                    <SelectItem value="openai_chat">
                      {t("providerForm.apiFormatOpenAIChat", {
                        defaultValue: "OpenAI Chat Completions",
                      })}
                    </SelectItem>
                    <SelectItem value="openai_responses">
                      {t("providerForm.apiFormatOpenAIResponses", {
                        defaultValue: "OpenAI Responses API",
                      })}
                    </SelectItem>
                    <SelectItem value="gemini_native">
                      {t("providerForm.apiFormatGeminiNative", {
                        defaultValue: "Gemini Native generateContent",
                      })}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <p className="text-xs leading-relaxed text-muted-foreground">
                  {t("providerForm.protocolBridgeHint", {
                    defaultValue:
                      "选择供应商实际提供的上游协议。不同协议需要开启路由接管，由本地代理转换；相同协议直接转发。",
                  })}
                </p>
              </div>
            )}

            {/* 认证字段选择器 */}
            <div className="space-y-2">
              <FormLabel>
                {t("providerForm.authField", { defaultValue: "认证字段" })}
              </FormLabel>
              <Select
                value={apiKeyField}
                onValueChange={(v) =>
                  onApiKeyFieldChange(v as ClaudeApiKeyField)
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="ANTHROPIC_AUTH_TOKEN">
                    {t("providerForm.authFieldAuthToken", {
                      defaultValue: "ANTHROPIC_AUTH_TOKEN（默认）",
                    })}
                  </SelectItem>
                  <SelectItem value="ANTHROPIC_API_KEY">
                    {t("providerForm.authFieldApiKey", {
                      defaultValue: "ANTHROPIC_API_KEY",
                    })}
                  </SelectItem>
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {t("providerForm.authFieldHint", {
                  defaultValue: "选择写入配置的认证环境变量名",
                })}
              </p>
            </div>

            {/* 模型映射 */}
            <div className="space-y-1 border-t border-border-default pt-2">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <FormLabel>{t("providerForm.modelMappingLabel")}</FormLabel>
                <div className="flex flex-wrap gap-2">
                  {/* 一键设置按钮 */}
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={handleQuickSet}
                    disabled={
                      modelFetchLoading ||
                      importingModels ||
                      (!availableModels.length && !canFetchModels)
                    }
                    className="h-7 gap-1"
                  >
                    {modelFetchLoading || importingModels ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Wand2 className="h-3.5 w-3.5" />
                    )}
                    {t("providerForm.quickSetModels", {
                      defaultValue: "一键设置",
                    })}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => handleModelFetchClick()}
                    disabled={modelFetchLoading}
                    className="h-7 gap-1"
                  >
                    {modelFetchLoading ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Download className="h-3.5 w-3.5" />
                    )}
                    {t("providerForm.fetchModels")}
                  </Button>
                </div>
              </div>
              <p className="text-xs text-muted-foreground">
                {t("providerForm.modelMappingHint")}
              </p>
            </div>

            <div className="space-y-2">
              <p className="text-xs text-muted-foreground">
                {t("providerForm.importModelsHint", {
                  defaultValue:
                    "One-click setup imports every model below. Claude Code 2.1.242+ displays these entries in /model; restart Claude Code after saving.",
                })}
              </p>
              {modelPickerOptions.map((option, index) => (
                <div
                  key={index}
                  className="grid grid-cols-[1fr_1fr_36px] gap-2"
                >
                  <Input
                    value={option.label ?? ""}
                    aria-label={t("providerForm.importModelName", {
                      defaultValue: "Display name",
                    })}
                    onChange={(event) =>
                      onModelPickerOptionsChange(
                        modelPickerOptions.map((row, i) =>
                          i === index
                            ? { ...row, label: event.target.value }
                            : row,
                        ),
                      )
                    }
                  />
                  <Input
                    value={option.model}
                    aria-label={t("providerForm.importModelId", {
                      defaultValue: "Request model",
                    })}
                    onChange={(event) =>
                      onModelPickerOptionsChange(
                        modelPickerOptions.map((row, i) =>
                          i === index
                            ? { ...row, model: event.target.value }
                            : row,
                        ),
                      )
                    }
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    aria-label={t("providerForm.removeImportedModel", {
                      defaultValue: "Remove model",
                    })}
                    onClick={() =>
                      onModelPickerOptionsChange(
                        modelPickerOptions.filter((_, i) => i !== index),
                      )
                    }
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>

            <div className="space-y-3">
              <div className="hidden grid-cols-[120px_1fr_minmax(0,1fr)_104px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
                <span>
                  {t("providerForm.modelRoleLabel", {
                    defaultValue: "模型角色",
                  })}
                </span>
                <span>
                  {t("providerForm.modelDisplayNameLabel", {
                    defaultValue: "显示名称",
                  })}
                </span>
                <span>
                  {t("providerForm.requestModelLabel", {
                    defaultValue: "实际请求模型",
                  })}
                </span>
                <span>
                  {t("providerForm.modelOneMHeader", {
                    defaultValue: "声明支持 1M",
                  })}
                </span>
              </div>

              {modelRoleRows.map((row) => {
                const modelBase = stripClaudeOneMMarker(row.model);
                const usesOneM =
                  row.supportsOneM && hasClaudeOneMMarker(row.model);

                return (
                  <div
                    key={row.role}
                    className="grid grid-cols-1 gap-2 md:grid-cols-[120px_1fr_minmax(0,1fr)_104px]"
                  >
                    <div className="flex h-9 items-center rounded-md border border-input bg-muted px-3 text-sm font-medium text-muted-foreground">
                      {row.label}
                    </div>
                    {row.displayNameField ? (
                      <Input
                        value={row.displayName ?? ""}
                        onChange={(event) =>
                          onModelChange(
                            row.displayNameField!,
                            event.target.value,
                          )
                        }
                        placeholder={
                          modelBase ||
                          t("providerForm.modelDisplayNamePlaceholder", {
                            defaultValue: "例如 DeepSeek V4 Pro",
                          })
                        }
                        autoComplete="off"
                      />
                    ) : (
                      <div className="flex h-9 items-center rounded-md border border-input bg-muted px-3 text-sm text-muted-foreground">
                        {t("providerForm.modelNoDisplayName", {
                          defaultValue: "不显示在 /model 菜单",
                        })}
                      </div>
                    )}
                    {renderModelInput(
                      row.inputId,
                      modelBase,
                      row.modelField,
                      t("providerForm.modelPlaceholder", { defaultValue: "" }),
                      (value) =>
                        handleRoleModelChange(
                          row,
                          row.supportsOneM
                            ? setClaudeOneMMarker(value, usesOneM)
                            : stripClaudeOneMMarker(value),
                        ),
                    )}
                    {row.supportsOneM && (
                      <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                        <Checkbox
                          checked={usesOneM}
                          onCheckedChange={(checked) =>
                            handleRoleOneMChange(row, checked === true)
                          }
                        />
                        {t("providerForm.modelOneMLabel", {
                          defaultValue: "1M",
                        })}
                      </label>
                    )}
                  </div>
                );
              })}
            </div>

            <div className="space-y-2 border-t border-border-default pt-4">
              <FormLabel htmlFor="claudeModel">
                {t("providerForm.fallbackModelLabel", {
                  defaultValue: "默认兜底模型",
                })}
              </FormLabel>
              <div className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_minmax(0,104px)]">
                {renderModelInput(
                  "claudeModel",
                  stripClaudeOneMMarker(claudeModel),
                  "ANTHROPIC_MODEL",
                  t("providerForm.modelPlaceholder", { defaultValue: "" }),
                  (value) =>
                    onModelChange(
                      "ANTHROPIC_MODEL",
                      setClaudeOneMMarker(value, fallbackUsesOneM),
                    ),
                )}
                <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                  <Checkbox
                    checked={fallbackUsesOneM}
                    onCheckedChange={(checked) => {
                      const base = stripClaudeOneMMarker(claudeModel).trim();
                      if (!base) return;
                      onModelChange(
                        "ANTHROPIC_MODEL",
                        setClaudeOneMMarker(base, checked === true),
                      );
                    }}
                  />
                  {t("providerForm.modelOneMLabel", {
                    defaultValue: "1M",
                  })}
                </label>
              </div>
              <p className="text-xs text-muted-foreground">
                {t("providerForm.fallbackModelHint", {
                  defaultValue:
                    "用于未明确落到 Sonnet、Opus、Fable、Haiku 角色的请求。使用第三方/中转端点时建议填写：否则这些请求（含 Haiku 后台子任务）会以原始 Claude 模型名透传给上游，可能因上游无此模型而报错。官方端点可留空。",
                })}
              </p>
            </div>

            <CustomUserAgentField
              id="claude-custom-user-agent"
              value={customUserAgent}
              onChange={onCustomUserAgentChange}
            />

            <div className="border-t border-border-default pt-3">
              <LocalProxyRequestOverridesField
                headersJson={localProxyHeadersOverride}
                bodyJson={localProxyBodyOverride}
                onHeadersJsonChange={onLocalProxyHeadersOverrideChange}
                onBodyJsonChange={onLocalProxyBodyOverrideChange}
              />
            </div>
          </CollapsibleContent>
        </Collapsible>
      )}
      {configurationDetection && (
        <ProviderConfigDetection
          {...configurationDetection}
          baseUrl={baseUrl}
          apiKey={apiKey}
          model={detectionModel}
          apiFormat={apiFormat}
          anthropicAuthField={apiKeyField}
          onModelChange={(model) => onModelChange("ANTHROPIC_MODEL", model)}
          onApiFormatChange={
            configurationDetection.onApiFormatChange ?? onApiFormatChange
          }
          catalogModels={fetchedModels}
          onCatalogModelsChange={setFetchedModels}
        />
      )}
    </>
  );
}
