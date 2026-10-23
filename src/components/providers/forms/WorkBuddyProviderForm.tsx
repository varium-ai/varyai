import { ConnectionPresetPicker } from "./ConnectionPresetPicker";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { toast } from "sonner";
import { Download, Loader2, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import JsonEditor from "@/components/JsonEditor";
import {
  providersApi,
  type WorkBuddySettingsSnapshot,
} from "@/lib/api/providers";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { useDarkMode } from "@/hooks/useDarkMode";
import ApiKeyInput from "./ApiKeyInput";
import { BasicFormFields } from "./BasicFormFields";
import { ModelInputWithFetch } from "./shared/ModelInputWithFetch";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderFormProps } from "./ProviderForm";

export function WorkBuddyProviderForm({
  initialData,
  onSubmit,
  onAddThirdParty,
  onCancel,
  submitLabel,
  showButtons = true,
  onSubmittingChange,
  onSubmitReadyChange,
  desktopClient = "workbuddy",
}: ProviderFormProps & { desktopClient?: "workbuddy" | "qoder" }) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const [isOfficial, setIsOfficial] = useState(
    initialData?.category === "official",
  );
  const native = initialData?.settingsConfig ?? {};
  const text = (key: string) =>
    typeof native[key] === "string" ? (native[key] as string) : "";
  const [protocol, setProtocol] = useState(text("protocol") || "openai");
  const [modelId, setModelId] = useState(text("id"));
  const [additionalModels, setAdditionalModels] = useState<string[]>(() =>
    Array.isArray(native.models)
      ? native.models
          .filter((model) => model.id !== text("id"))
          .map((model) => String(model.id ?? ""))
      : [],
  );
  const [url, setUrl] = useState(text("url"));
  const [apiKey, setApiKey] = useState(text("apiKey"));
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const modelFetchSequence = useRef(0);
  const [vendor, setVendor] = useState(text("vendor") || "OpenAI");
  const [maxInputTokens, setMaxInputTokens] = useState(
    native.maxInputTokens?.toString() ?? "",
  );
  const [maxOutputTokens, setMaxOutputTokens] = useState(
    native.maxOutputTokens?.toString() ?? "",
  );
  const [supportsToolCall, setSupportsToolCall] = useState(
    native.supportsToolCall === true,
  );
  const [supportsImages, setSupportsImages] = useState(
    native.supportsImages === true,
  );
  const [toolCallChanged, setToolCallChanged] = useState(false);
  const [imagesChanged, setImagesChanged] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [settingsSnapshot, setSettingsSnapshot] =
    useState<WorkBuddySettingsSnapshot | null>(null);
  const [settingsContent, setSettingsContent] = useState("");
  const [settingsError, setSettingsError] = useState("");
  const [settingsReload, setSettingsReload] = useState(0);
  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues: {
      name: initialData?.name ?? text("name"),
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: "{}",
    },
  });

  useEffect(() => {
    if (!isOfficial) return;
    let cancelled = false;
    setSettingsSnapshot(null);
    setSettingsError("");
    (desktopClient === "qoder"
      ? providersApi.getQoderSettings()
      : providersApi.getWorkBuddySettings()
    )
      .then((snapshot) => {
        if (cancelled) return;
        setSettingsSnapshot(snapshot);
        setSettingsContent(snapshot.content);
      })
      .catch((error) => {
        if (!cancelled) setSettingsError(extractErrorMessage(error));
      });
    return () => {
      cancelled = true;
    };
  }, [isOfficial, settingsReload, desktopClient]);
  useEffect(() => {
    onSubmitReadyChange?.(!isOfficial || settingsSnapshot !== null);
  }, [isOfficial, settingsSnapshot, onSubmitReadyChange]);
  useEffect(() => {
    onSubmittingChange?.(submitting);
  }, [onSubmittingChange, submitting]);

  useEffect(() => {
    setFetchedModels([]);
    setIsFetchingModels(false);
    return () => {
      modelFetchSequence.current += 1;
    };
  }, [url, apiKey, isOfficial]);

  const handleFetchModels = async () => {
    if (!url.trim() || !apiKey.trim()) {
      showFetchModelsError(null, t, {
        hasBaseUrl: !!url.trim(),
        hasApiKey: !!apiKey.trim(),
      });
      return;
    }
    let endpoint: URL;
    try {
      endpoint = new URL(url.trim());
      if (
        !["http:", "https:"].includes(endpoint.protocol) ||
        (desktopClient === "qoder" &&
          !!(
            endpoint.username ||
            endpoint.password ||
            endpoint.search ||
            endpoint.hash
          ))
      )
        throw new Error();
    } catch {
      toast.error(t(`${desktopClient}.form.invalidUrl`));
      return;
    }
    const path = endpoint.pathname
      .replace(/\/+$/, "")
      .replace(/\/chat\/completions$/, "");
    const baseUrl = `${endpoint.origin}${path}`;
    const sequence = ++modelFetchSequence.current;
    setIsFetchingModels(true);
    setFetchedModels([]);
    try {
      const models = await fetchModelsForConfig(baseUrl, apiKey.trim());
      if (sequence !== modelFetchSequence.current) return;
      setFetchedModels(models);
      if (models.length === 0) {
        toast.info(t("providerForm.fetchModelsEmpty"));
      } else {
        toast.success(
          t("providerForm.fetchModelsSuccess", { count: models.length }),
        );
      }
    } catch (error) {
      if (sequence === modelFetchSequence.current)
        showFetchModelsError(error, t);
    } finally {
      if (sequence === modelFetchSequence.current) setIsFetchingModels(false);
    }
  };

  const save = async (data: ProviderFormData) => {
    if (submitting) return;
    const name = data.name.trim();
    const addThirdParty =
      isOfficial &&
      !!onAddThirdParty &&
      [url, apiKey, modelId, ...additionalModels].some((value) => value.trim());
    const needsModel = !isOfficial || addThirdParty;
    const modelIds = [modelId, ...additionalModels].map((id) => id.trim());
    const missingFields: string[] = [];
    if (!name) missingFields.push(t("provider.name"));
    if (needsModel) {
      if (modelIds.some((id) => !id))
        missingFields.push(t(`${desktopClient}.form.modelId`));
      if (!url.trim()) missingFields.push("URL");
      if (!apiKey.trim()) missingFields.push(t("universalProvider.apiKey"));
    }
    if (missingFields.length > 0) {
      toast.error(
        t("providerForm.missingRequiredFields", {
          fields: missingFields.join(", "),
        }),
      );
      return;
    }
    let config: Record<string, unknown> = {};
    if (isOfficial) {
      if (!settingsSnapshot) return;
      try {
        const parsed = JSON.parse(settingsContent);
        if (
          !parsed ||
          Array.isArray(parsed) ||
          typeof parsed !== "object" ||
          (parsed.env !== undefined &&
            (!parsed.env ||
              Array.isArray(parsed.env) ||
              typeof parsed.env !== "object"))
        )
          throw new Error();
      } catch {
        toast.error(t(`${desktopClient}.settings.invalid`));
        return;
      }
    }
    if (needsModel) {
      if (new Set(modelIds).size !== modelIds.length) {
        toast.error(t(`${desktopClient}.form.duplicateModels`));
        return;
      }
      let endpoint: URL;
      try {
        endpoint = new URL(url.trim());
        if (
          !["http:", "https:"].includes(endpoint.protocol) ||
          (desktopClient === "qoder" &&
            !!(
              endpoint.username ||
              endpoint.password ||
              endpoint.search ||
              endpoint.hash
            ))
        )
          throw new Error();
      } catch {
        toast.error(t(`${desktopClient}.form.invalidUrl`));
        return;
      }
      const path = endpoint.pathname.replace(/\/+$/, "");
      endpoint.pathname = path.endsWith("/chat/completions")
        ? path
        : `${path}/chat/completions`;
      const modelName = addThirdParty ? modelId.trim() : name;
      config = {
        ...native,
        id: modelId.trim(),
        name: modelName,
        vendor: vendor.trim() || "OpenAI",
        url: endpoint.toString(),
        apiKey: apiKey.trim(),
        ...(desktopClient === "qoder" ? { protocol } : {}),
      };
      if (!initialData || addThirdParty || toolCallChanged)
        config.supportsToolCall = supportsToolCall;
      if (!initialData || addThirdParty || imagesChanged)
        config.supportsImages = supportsImages;
      for (const [key, value] of [
        ["maxInputTokens", maxInputTokens],
        ["maxOutputTokens", maxOutputTokens],
      ]) {
        if (!value.trim()) {
          delete config[key];
        } else if (
          !/^\d+$/.test(value) ||
          !Number.isSafeInteger(Number(value)) ||
          Number(value) <= 0
        ) {
          toast.error(t(`${desktopClient}.form.invalidTokens`));
          return;
        } else {
          config[key] = Number(value);
        }
      }
      if (modelIds.length > 1 || Array.isArray(native.models)) {
        config.models = modelIds.map((id) => ({
          ...(Array.isArray(native.models)
            ? native.models.find((model) => model.id === id)
            : {}),
          id,
          name: modelIds.length > 1 ? `${modelName} · ${id}` : modelName,
        }));
      }
    }
    setSubmitting(true);
    try {
      if (addThirdParty && onAddThirdParty) {
        try {
          await onAddThirdParty({
            ...data,
            name: modelId.trim(),
            settingsConfig: JSON.stringify(config, null, 2),
            presetCategory: "custom",
          });
        } catch (error) {
          toast.error(
            t("desktopModels.saveFailed", {
              error: extractErrorMessage(error),
            }),
          );
          return;
        }
        setApiKey("");
        setUrl("");
        setModelId("");
        setAdditionalModels([]);
      }
      if (
        isOfficial &&
        settingsSnapshot &&
        (!addThirdParty || settingsContent !== settingsSnapshot.content)
      ) {
        try {
          await (
            desktopClient === "qoder"
              ? providersApi.saveQoderSettings
              : providersApi.saveWorkBuddySettings
          )(settingsSnapshot.revision, settingsContent);
        } catch (error) {
          toast.error(
            t(`${desktopClient}.settings.saveFailed`, {
              error: extractErrorMessage(error),
            }),
          );
          return;
        }
      }
      await onSubmit({
        ...data,
        name,
        settingsConfig: JSON.stringify(isOfficial ? {} : config, null, 2),
        presetCategory: isOfficial ? "official" : "custom",
        meta: initialData?.meta,
      });
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Form {...form}>
      <form
        id="provider-form"
        className="space-y-6 glass rounded-xl p-6"
        onSubmit={form.handleSubmit(save)}
      >
        <ConnectionPresetPicker
          supportedFormats={
            desktopClient === "qoder"
              ? ["openai_chat", "openai_responses", "anthropic"]
              : ["openai_chat"]
          }
          disabled={isOfficial && !onAddThirdParty}
          current={{ name: form.watch("name"), baseUrl: url, apiKey }}
          onApply={(preset) => {
            if (
              desktopClient === "qoder" &&
              preset.apiFormat &&
              preset.apiFormat !== "gemini_native"
            )
              setProtocol(
                {
                  anthropic: "anthropic",
                  openai_chat: "openai",
                  openai_responses: "openai-responses",
                }[preset.apiFormat],
              );
            form.setValue("name", preset.name);
            setUrl(preset.baseUrl);
            setApiKey(preset.apiKey);
          }}
        />
        <BasicFormFields form={form} defaultIcon={desktopClient} />
        {!initialData && (
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={isOfficial}
              onChange={(event) => {
                setIsOfficial(event.target.checked);
                if (event.target.checked && !form.getValues("name").trim())
                  form.setValue("name", t(`${desktopClient}.officialName`));
              }}
            />
            {t("desktopModels.officialLogin")}
          </label>
        )}
        {(!isOfficial || !!onAddThirdParty) && (
          <>
            {isOfficial && (
              <h3 className="font-medium">
                {t("desktopModels.optionalThirdParty")}
              </h3>
            )}
            {isOfficial && (
              <p className="text-sm text-muted-foreground">
                {t("desktopModels.officialAndCustom")}
              </p>
            )}
            <p className="text-sm text-muted-foreground">
              {t(`${desktopClient}.form.description`)}
            </p>
            {desktopClient === "qoder" && (
              <div className="space-y-2">
                <Label htmlFor="qoder-protocol">
                  {t("qoder.form.protocol")}
                </Label>
                <Select value={protocol} onValueChange={setProtocol}>
                  <SelectTrigger
                    id="qoder-protocol"
                    className="h-11 rounded-xl px-4 text-sm"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent className="rounded-xl">
                    <SelectItem
                      value="openai"
                      className="min-h-11 rounded-lg pr-4"
                    >
                      OpenAI Chat Completions
                    </SelectItem>
                    <SelectItem
                      value="openai-responses"
                      className="min-h-11 rounded-lg pr-4"
                    >
                      OpenAI Responses
                    </SelectItem>
                    <SelectItem
                      value="anthropic"
                      className="min-h-11 rounded-lg pr-4"
                    >
                      Anthropic Messages
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
            )}
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <Label htmlFor="workbuddy-model-id">
                    {t(`${desktopClient}.form.modelId`)}
                  </Label>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-7 gap-1"
                    onClick={handleFetchModels}
                    disabled={isFetchingModels}
                  >
                    {isFetchingModels ? (
                      <Loader2 className="h-3 w-3 animate-spin" />
                    ) : (
                      <Download className="h-3 w-3" />
                    )}
                    {t("providerForm.fetchModels")}
                  </Button>
                </div>
                <ModelInputWithFetch
                  id="workbuddy-model-id"
                  value={modelId}
                  onChange={setModelId}
                  fetchedModels={fetchedModels}
                  isLoading={isFetchingModels}
                />
                {additionalModels.map((id, index) => (
                  <div key={index} className="flex items-end gap-2">
                    <div className="min-w-0 flex-1 space-y-2">
                      <Label htmlFor={`workbuddy-model-id-${index}`}>
                        {t(`${desktopClient}.form.modelId`)} {index + 2}
                      </Label>
                      <ModelInputWithFetch
                        id={`workbuddy-model-id-${index}`}
                        value={id}
                        onChange={(value) =>
                          setAdditionalModels((models) =>
                            models.map((model, i) =>
                              i === index ? value : model,
                            ),
                          )
                        }
                        fetchedModels={fetchedModels}
                        isLoading={isFetchingModels}
                      />
                    </div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={`${t(`${desktopClient}.form.removeModel`)} ${index + 2}`}
                      onClick={() =>
                        setAdditionalModels((models) =>
                          models.filter((_, i) => i !== index),
                        )
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}
                <div className="flex flex-wrap gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() =>
                      setAdditionalModels((models) => [...models, ""])
                    }
                  >
                    <Plus className="mr-1 h-3 w-3" />
                    {t(`${desktopClient}.form.addModel`)}
                  </Button>
                  {fetchedModels.length > 0 && (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        const ids = Array.from(
                          new Set(
                            [
                              modelId,
                              ...additionalModels,
                              ...fetchedModels.map((model) => model.id),
                            ]
                              .map((id) => id.trim())
                              .filter(Boolean),
                          ),
                        );
                        setModelId(ids[0]);
                        setAdditionalModels(ids.slice(1));
                      }}
                    >
                      {t(`${desktopClient}.form.addFetchedModels`)}
                    </Button>
                  )}
                </div>
                <p className="text-xs text-muted-foreground">
                  {t(`${desktopClient}.form.sharedSettings`)}
                </p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="workbuddy-vendor">
                  {t(`${desktopClient}.form.vendor`)}
                </Label>
                <ImeSafeInput
                  id="workbuddy-vendor"
                  value={vendor}
                  onValueChange={setVendor}
                />
              </div>
              <div className="space-y-2 md:col-span-2">
                <Label htmlFor="workbuddy-url">
                  {t(`${desktopClient}.form.url`)}
                </Label>
                <ImeSafeInput
                  id="workbuddy-url"
                  value={url}
                  onValueChange={setUrl}
                  placeholder="https://api.example.com/v1/chat/completions"
                />
              </div>
              <div className="space-y-2 md:col-span-2">
                <ApiKeyInput
                  id="workbuddy-api-key"
                  label={t("universalProvider.apiKey")}
                  value={apiKey}
                  onChange={setApiKey}
                />
              </div>
              {(
                [
                  ["maxInputTokens", maxInputTokens, setMaxInputTokens],
                  ["maxOutputTokens", maxOutputTokens, setMaxOutputTokens],
                ] as const
              ).map(([key, value, setValue]) => (
                <div className="space-y-2" key={key}>
                  <Label htmlFor={`workbuddy-${key}`}>
                    {t(`${desktopClient}.form.${key}`)}
                  </Label>
                  <ImeSafeInput
                    id={`workbuddy-${key}`}
                    inputMode="numeric"
                    value={value}
                    onValueChange={setValue}
                  />
                </div>
              ))}
              <div className="flex items-center gap-2">
                <Switch
                  id="workbuddy-tool-call"
                  checked={supportsToolCall}
                  onCheckedChange={(value) => {
                    setSupportsToolCall(value);
                    setToolCallChanged(true);
                  }}
                />
                <Label htmlFor="workbuddy-tool-call">
                  {t(`${desktopClient}.form.toolCall`)}
                </Label>
              </div>
              <div className="flex items-center gap-2">
                <Switch
                  id="workbuddy-images"
                  checked={supportsImages}
                  onCheckedChange={(value) => {
                    setSupportsImages(value);
                    setImagesChanged(true);
                  }}
                />
                <Label htmlFor="workbuddy-images">
                  {t(`${desktopClient}.form.images`)}
                </Label>
              </div>
            </div>
          </>
        )}
        {isOfficial ? (
          <>
            <p className="text-sm text-muted-foreground">
              {t("desktopModels.officialLogin")}
            </p>
            <section className="space-y-3">
              <Label>{t(`${desktopClient}.settings.label`)}</Label>
              <p className="break-all font-mono text-xs text-muted-foreground">
                {settingsSnapshot?.path}
              </p>
              <p className="text-sm text-muted-foreground">
                {t(`${desktopClient}.settings.description`)}
              </p>
              {settingsError ? (
                <div role="alert" className="space-y-2">
                  <p>
                    {t(`${desktopClient}.settings.loadFailed`, {
                      error: settingsError,
                    })}
                  </p>
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => setSettingsReload((value) => value + 1)}
                  >
                    {t(`${desktopClient}.settings.reload`)}
                  </Button>
                </div>
              ) : !settingsSnapshot ? (
                <p role="status">{t("common.loading")}</p>
              ) : (
                <JsonEditor
                  ariaLabel={t(`${desktopClient}.settings.label`)}
                  value={settingsContent}
                  onChange={setSettingsContent}
                  darkMode={isDarkMode}
                  height={360}
                />
              )}
            </section>
          </>
        ) : null}
        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"
              disabled={submitting || (isOfficial && !settingsSnapshot)}
            >
              {submitLabel}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
