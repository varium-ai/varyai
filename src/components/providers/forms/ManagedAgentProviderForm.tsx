import { ConnectionPresetPicker } from "./ConnectionPresetPicker";
import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from "@/components/ui/select";
import {
  fetchModelsForConfig,
  showFetchModelsError,
} from "@/lib/api/model-fetch";
import type { ProviderFormProps } from "./ProviderForm";

export function ManagedAgentProviderForm({
  appId,
  initialData,
  onSubmit,
  onAddThirdParty,
  onCancel,
  submitLabel,
  showButtons = true,
  onSubmittingChange,
  onSubmitReadyChange,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const config = initialData?.settingsConfig ?? {};
  const official = initialData?.category === "official";
  const [name, setName] = useState(initialData?.name ?? "");
  const [baseUrl, setBaseUrl] = useState(String(config.baseUrl ?? ""));
  const [apiKey, setApiKey] = useState(String(config.apiKey ?? ""));
  const [protocol, setProtocol] = useState(String(config.protocol ?? "openai"));
  const [models, setModels] = useState(
    Array.isArray(config.models)
      ? config.models.map((m) => m.model).join("\n")
      : "",
  );
  const [saving, setSaving] = useState(false);
  const [fetching, setFetching] = useState(false);
  useEffect(() => {
    onSubmitReadyChange?.(true);
    return () => onSubmitReadyChange?.(false);
  }, [onSubmitReadyChange]);
  useEffect(() => {
    onSubmittingChange?.(saving);
  }, [saving, onSubmittingChange]);
  async function save(event: FormEvent) {
    event.preventDefault();
    if (saving) return;
    const custom =
      !official || !!(baseUrl.trim() || apiKey.trim() || models.trim());
    const ids = models
      .split(/[\n,]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (
      !name.trim() ||
      (custom &&
        (!baseUrl.trim() ||
          !apiKey.trim() ||
          !ids.length ||
          new Set(ids).size !== ids.length))
    ) {
      toast.error(t("managedAgent.required"));
      return;
    }
    if (custom) {
      try {
        const url = new URL(baseUrl);
        if (
          !["http:", "https:"].includes(url.protocol) ||
          url.username ||
          url.password ||
          url.search ||
          url.hash
        )
          throw new Error();
      } catch {
        toast.error(t("managedAgent.invalidUrl"));
        return;
      }
    }
    setSaving(true);
    try {
      const values = {
        name: name.trim(),
        settingsConfig: JSON.stringify({
          ...config,
          baseUrl: baseUrl.trim().replace(/\/$/, ""),
          apiKey: apiKey.trim(),
          protocol,
          models: ids.map((model) => ({
            ...(Array.isArray(config.models)
              ? config.models.find((saved) => saved.model === model)
              : undefined),
            model,
          })),
        }),
        presetCategory: "custom" as const,
      };
      if (official && custom) {
        if (!onAddThirdParty) throw new Error();
        await onAddThirdParty({ ...values, name: ids[0] });
        setBaseUrl("");
        setApiKey("");
        setModels("");
      }
      await onSubmit(
        official
          ? {
              name: name.trim(),
              settingsConfig: "{}",
              presetCategory: "official",
            }
          : values,
      );
    } catch {
      toast.error(t("managedAgent.saveFailed"));
    } finally {
      setSaving(false);
    }
  }
  async function fetchModels() {
    setFetching(true);
    try {
      const fetched = await fetchModelsForConfig(baseUrl.trim(), apiKey.trim());
      setModels(
        [
          ...new Set([
            ...models.split("\n").filter(Boolean),
            ...fetched.map((m) => m.id),
          ]),
        ].join("\n"),
      );
    } catch (error) {
      showFetchModelsError(error, t, {
        hasBaseUrl: !!baseUrl.trim(),
        hasApiKey: !!apiKey.trim(),
      });
    } finally {
      setFetching(false);
    }
  }
  return (
    <form
      id="provider-form"
      className="space-y-5 rounded-xl p-6"
      onSubmit={save}
    >
      <p className="text-sm text-muted-foreground">
        {t("managedAgent.description", { app: t(`apps.${appId}`) })}
      </p>
      {appId === "kimi" && (
        <p className="text-sm text-muted-foreground">
          {t("homeAudit.appProtectionRestart", { app: t("apps.kimi") })}
        </p>
      )}
      {appId === "trae" && (
        <p className="text-sm text-muted-foreground">
          {t("managedAgent.traePending")}
        </p>
      )}
      <ConnectionPresetPicker
        current={{ name, baseUrl, apiKey }}
        onApply={(preset) => {
          setName(preset.name);
          setBaseUrl(preset.baseUrl);
          setApiKey(preset.apiKey);
        }}
      />
      <div className="space-y-2">
        <Label htmlFor="agent-name">{t("provider.name")}</Label>
        <Input
          id="agent-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
      </div>
      {official && (
        <p className="text-sm text-muted-foreground">
          {t("desktopModels.officialAndCustom")}
        </p>
      )}
      <div className="space-y-2">
        <Label htmlFor="agent-base">{t("managedAgent.baseUrl")}</Label>
        <Input
          id="agent-base"
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
          placeholder="https://api.example.com/v1"
        />
      </div>
      <div className="space-y-2">
        <Label htmlFor="agent-key">{t("managedAgent.apiKey")}</Label>
        <Input
          id="agent-key"
          type="password"
          autoComplete="off"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
        />
      </div>
      <div className="space-y-2">
        <Label htmlFor="agent-protocol">{t("managedAgent.protocol")}</Label>
        <Select value={protocol} onValueChange={setProtocol}>
          <SelectTrigger id="agent-protocol" className="h-11 rounded-xl">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="openai">OpenAI Chat Completions</SelectItem>
            <SelectItem value="openai-responses">OpenAI Responses</SelectItem>
            <SelectItem value="anthropic">Anthropic Messages</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <Label htmlFor="agent-models">{t("managedAgent.models")}</Label>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={fetching || !baseUrl.trim() || !apiKey.trim()}
            onClick={fetchModels}
          >
            {t("managedAgent.fetchModels")}
          </Button>
        </div>
        <Textarea
          id="agent-models"
          value={models}
          onChange={(e) => setModels(e.target.value)}
          placeholder={t("managedAgent.modelsHint")}
        />
      </div>
      {showButtons && (
        <div className="flex justify-end gap-3">
          <Button type="button" variant="outline" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" disabled={saving}>
            {submitLabel}
          </Button>
        </div>
      )}
    </form>
  );
}
