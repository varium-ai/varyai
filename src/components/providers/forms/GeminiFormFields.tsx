import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Download, Info, Loader2, Trash2, Wand2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { toast } from "sonner";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { ProviderCategory, ClaudeApiFormat } from "@/types";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface EndpointCandidate {
  url: string;
}

type GeminiModelConfig = Record<string, unknown> & {
  experimental?: Record<string, unknown>;
  modelConfigs?: Record<string, unknown> & {
    modelDefinitions?: Record<string, Record<string, unknown>>;
  };
};

function parseModelConfig(value: string): GeminiModelConfig | null {
  const isObject = (item: unknown): item is Record<string, unknown> =>
    !!item && typeof item === "object" && !Array.isArray(item);
  try {
    const config = JSON.parse(value.trim() || "{}");
    if (!isObject(config)) return null;
    if (config.experimental !== undefined && !isObject(config.experimental))
      return null;
    if (config.modelConfigs !== undefined) {
      if (!isObject(config.modelConfigs)) return null;
      const definitions = config.modelConfigs.modelDefinitions;
      if (
        definitions !== undefined &&
        (!isObject(definitions) || !Object.values(definitions).every(isObject))
      )
        return null;
    }
    return config as GeminiModelConfig;
  } catch {
    return null;
  }
}

interface GeminiFormFieldsProps {
  apiFormat?: ClaudeApiFormat;
  onApiFormatChange?: (format: ClaudeApiFormat) => void;
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

  // Base URL
  shouldShowSpeedTest: boolean;
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;

  // Model
  shouldShowModelField: boolean;
  model: string;
  onModelChange: (value: string) => void;
  configValue: string;
  onConfigChange: (value: string) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];
}

export function GeminiFormFields({
  apiFormat = "gemini_native",
  onApiFormatChange,
  providerId,
  shouldShowApiKey,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  shouldShowSpeedTest,
  baseUrl,
  onBaseUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  shouldShowModelField,
  model,
  onModelChange,
  configValue,
  onConfigChange,
  speedTestEndpoints,
}: GeminiFormFieldsProps) {
  const { t } = useTranslation();

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [importingModels, setImportingModels] = useState(false);
  const fetchSequence = useRef(0);
  const latest = useRef({ configValue, model });
  latest.current = { configValue, model };
  const config = parseModelConfig(configValue);
  const definitions = config?.modelConfigs?.modelDefinitions ?? {};
  const modelSuggestions = Array.from(
    new Map([
      ...Object.keys(definitions).map((id): [string, FetchedModel] => [
        id,
        { id, ownedBy: null },
      ]),
      ...fetchedModels.map((item): [string, FetchedModel] => [item.id, item]),
    ]).values(),
  );

  useEffect(() => {
    fetchSequence.current += 1;
    setFetchedModels([]);
    setIsFetchingModels(false);
    return () => {
      fetchSequence.current += 1;
    };
  }, [baseUrl, apiKey, apiFormat]);

  const handleFetchModels = useCallback(
    async (importModels = false) => {
      if (!baseUrl || !apiKey) {
        showFetchModelsError(null, t, {
          hasApiKey: !!apiKey,
          hasBaseUrl: !!baseUrl,
        });
        return;
      }
      const sequence = ++fetchSequence.current;
      setIsFetchingModels(true);
      try {
        const models = await fetchModelsForConfig(
          baseUrl,
          apiKey,
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
        if (sequence !== fetchSequence.current) return;
        setFetchedModels(models);
        if (!importModels && models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else if (!importModels) {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: models.length }),
          );
        }
        return models;
      } catch (err) {
        if (sequence === fetchSequence.current) showFetchModelsError(err, t);
      } finally {
        if (sequence === fetchSequence.current) setIsFetchingModels(false);
      }
    },
    [baseUrl, apiKey, apiFormat, t],
  );

  async function handleQuickSet() {
    setImportingModels(true);
    try {
      const models = fetchedModels.length
        ? fetchedModels
        : await handleFetchModels(true);
      if (!models) return;
      const current = parseModelConfig(latest.current.configValue);
      if (!current) return;
      const next: Record<string, Record<string, unknown>> = Object.assign(
        Object.create(null),
        current.modelConfigs?.modelDefinitions,
      );
      const ids = [
        ...new Set(models.map(({ id }) => id.trim()).filter(Boolean)),
      ];
      if (!ids.length) {
        toast.info(t("providerForm.fetchModelsEmpty"));
        return;
      }
      for (const id of ids) {
        if (!Object.prototype.hasOwnProperty.call(next, id))
          next[id] = { displayName: id, isVisible: true };
      }
      onConfigChange(
        JSON.stringify(
          {
            ...current,
            experimental: {
              ...current.experimental,
              dynamicModelConfiguration: true,
            },
            modelConfigs: { ...current.modelConfigs, modelDefinitions: next },
          },
          null,
          2,
        ),
      );
      if (!latest.current.model.trim()) onModelChange(ids[0]);
      toast.success(
        t("providerForm.quickSetSuccess", { count: Object.keys(next).length }),
      );
    } finally {
      setImportingModels(false);
    }
  }

  function updateDefinitions(next: typeof definitions) {
    if (!config) return;
    onConfigChange(
      JSON.stringify(
        {
          ...config,
          modelConfigs: { ...config.modelConfigs, modelDefinitions: next },
        },
        null,
        2,
      ),
    );
  }

  // 检测是否为 Google 官方（使用 OAuth）
  const isGoogleOfficial =
    partnerPromotionKey?.toLowerCase() === "google-official";

  return (
    <>
      {category !== "official" && !isGoogleOfficial && onApiFormatChange && (
        <div className="space-y-2">
          <FormLabel htmlFor="gemini-upstream-format">
            {t("providerForm.apiFormat")}
          </FormLabel>
          <Select
            value={apiFormat}
            onValueChange={(value) =>
              onApiFormatChange(value as ClaudeApiFormat)
            }
          >
            <SelectTrigger id="gemini-upstream-format">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(
                [
                  "anthropic",
                  "openai_chat",
                  "openai_responses",
                  "gemini_native",
                ] as ClaudeApiFormat[]
              ).map((format, index) => (
                <SelectItem key={format} value={format}>
                  {t(
                    [
                      "providerForm.apiFormatAnthropic",
                      "providerForm.apiFormatOpenAIChat",
                      "providerForm.apiFormatOpenAIResponses",
                      "providerForm.apiFormatGeminiNative",
                    ][index],
                  )}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            {t("providerForm.protocolBridgeHint")}
          </p>
        </div>
      )}
      {/* Google OAuth 提示 */}
      {isGoogleOfficial && (
        <div className="rounded-lg border border-blue-200 bg-blue-50 p-4 dark:border-blue-800 dark:bg-blue-950">
          <div className="flex gap-3">
            <Info className="h-5 w-5 flex-shrink-0 text-blue-600 dark:text-blue-400" />
            <div className="space-y-1">
              <p className="text-sm font-medium text-blue-900 dark:text-blue-100">
                {t("provider.form.gemini.oauthTitle", {
                  defaultValue: "OAuth 认证模式",
                })}
              </p>
              <p className="text-sm text-blue-700 dark:text-blue-300">
                {t("provider.form.gemini.oauthHint", {
                  defaultValue:
                    "Google 官方使用 OAuth 个人认证，无需填写 API Key。首次使用时会自动打开浏览器进行登录。",
                })}
              </p>
            </div>
          </div>
        </div>
      )}

      {/* API Key 输入框 */}
      {shouldShowApiKey && !isGoogleOfficial && (
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

      {/* Base URL 输入框（统一使用与 Codex 相同的样式与交互） */}
      {shouldShowSpeedTest && (
        <EndpointField
          id="baseUrl"
          label={t("providerForm.apiEndpoint", { defaultValue: "API 端点" })}
          value={baseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.apiEndpointPlaceholder", {
            defaultValue: "https://your-api-endpoint.com/",
          })}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {/* Model 输入框 */}
      {shouldShowModelField && (
        <div className="space-y-2">
          <FormLabel htmlFor="gemini-model">
            {t("geminiConfig.defaultModel")}
          </FormLabel>
          <ModelInputWithFetch
            id="gemini-model"
            value={model}
            onChange={onModelChange}
            placeholder="gemini-3.6-flash"
            fetchedModels={modelSuggestions}
            isLoading={isFetchingModels}
          />
          <p className="text-xs text-muted-foreground">
            {t("geminiConfig.defaultModelHint")}
          </p>
          <div className="flex flex-wrap items-center justify-between gap-2 border-t border-border pt-3">
            <p className="text-sm font-medium">
              {t("geminiConfig.selectableModels")}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleQuickSet}
                disabled={
                  !config ||
                  isFetchingModels ||
                  importingModels ||
                  (!fetchedModels.length && (!baseUrl.trim() || !apiKey.trim()))
                }
                className="h-7 gap-1"
              >
                {isFetchingModels || importingModels ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Wand2 className="h-3.5 w-3.5" />
                )}
                {t("providerForm.quickSetModels")}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => handleFetchModels()}
                disabled={isFetchingModels || importingModels}
                className="h-7 gap-1"
              >
                {isFetchingModels ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Download className="h-3.5 w-3.5" />
                )}
                {t("providerForm.fetchModels")}
              </Button>
            </div>
          </div>
          <p className="text-xs text-muted-foreground">
            {t("geminiConfig.importModelsHint")}
          </p>
          {!config && (
            <p className="text-xs text-destructive">
              {t("geminiConfig.modelsConfigInvalid")}
            </p>
          )}
          {Object.keys(definitions).length > 0 && (
            <div className="grid grid-cols-[1fr_1fr_36px] gap-2 text-xs text-muted-foreground">
              <span>{t("providerForm.importModelName")}</span>
              <span>{t("providerForm.importModelId")}</span>
            </div>
          )}
          {Object.entries(definitions).map(([id, definition]) => (
            <div key={id} className="grid grid-cols-[1fr_1fr_36px] gap-2">
              <Input
                value={
                  typeof definition.displayName === "string"
                    ? definition.displayName
                    : id
                }
                aria-label={t("providerForm.importModelName")}
                onChange={(event) =>
                  updateDefinitions({
                    ...definitions,
                    [id]: { ...definition, displayName: event.target.value },
                  })
                }
              />
              <Input
                value={id}
                readOnly
                aria-label={t("providerForm.importModelId")}
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label={t("providerForm.removeImportedModel")}
                onClick={() => {
                  const next = { ...definitions };
                  delete next[id];
                  updateDefinitions(next);
                }}
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>
          ))}
        </div>
      )}

      {/* 端点测速弹窗 */}
      {shouldShowSpeedTest && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId="gemini"
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
    </>
  );
}
