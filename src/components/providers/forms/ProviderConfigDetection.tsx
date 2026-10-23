import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ModelDropdown } from "./shared/ModelDropdown";
import { fetchModelsForConfig, type FetchedModel } from "@/lib/api/model-fetch";
import {
  detectProviderProtocols,
  type ProtocolDetection,
} from "@/lib/api/provider-protocol-detection";
import type { ClaudeApiFormat, ClaudeApiKeyField } from "@/types";

interface Props {
  baseUrl: string;
  apiKey: string;
  model: string;
  apiFormat: ClaudeApiFormat;
  anthropicAuthField?: ClaudeApiKeyField;
  editingCredentials: boolean;
  autoSelectFormat: boolean;
  onModelChange: (model: string) => void;
  onApiFormatChange: (format: ClaudeApiFormat) => void;
  onManualFormatSelect?: () => void;
  catalogModels?: FetchedModel[];
  onCatalogModelsChange?: (models: FetchedModel[]) => void;
}
const labels: Record<ClaudeApiFormat, string> = {
  anthropic: "providerForm.apiFormatAnthropic",
  openai_chat: "providerForm.apiFormatOpenAIChat",
  openai_responses: "providerForm.apiFormatOpenAIResponses",
  gemini_native: "providerForm.apiFormatGeminiNative",
};
function validBaseUrl(value: string) {
  try {
    const url = new URL(value);
    return (
      ["http:", "https:"].includes(url.protocol) &&
      !url.username &&
      !url.password &&
      !url.search &&
      !url.hash
    );
  } catch {
    return false;
  }
}

export function ProviderConfigDetection(props: Props) {
  const { t } = useTranslation();
  const { baseUrl, apiKey, model, editingCredentials, anthropicAuthField } =
    props;
  const [localModels, setLocalModels] = useState<FetchedModel[]>([]);
  const models = props.catalogModels ?? localModels;
  const setModels = props.onCatalogModelsChange ?? setLocalModels;
  const [discovering, setDiscovering] = useState(false);
  const [checking, setChecking] = useState(false);
  const [results, setResults] = useState<ProtocolDetection[]>([]);
  const [checkError, setCheckError] = useState<string | null>(null);
  const sequence = useRef(0);
  const latest = useRef(props);
  latest.current = props;
  const completedCatalog = useRef<{ baseUrl: string; apiKey: string } | null>(
    null,
  );
  const configured = validBaseUrl(baseUrl) && !!apiKey.trim();

  useEffect(() => {
    setModels([]);
    setDiscovering(false);
    completedCatalog.current = null;
  }, [baseUrl, apiKey, setModels]);

  useEffect(() => {
    let active = true;
    const completed = completedCatalog.current;
    if (
      editingCredentials ||
      !configured ||
      (completed?.baseUrl === baseUrl && completed.apiKey === apiKey)
    )
      return;
    const timer = window.setTimeout(() => {
      setDiscovering(true);
      fetchModelsForConfig(baseUrl.trim(), apiKey.trim())
        .then((catalog) => {
          if (active) setModels(catalog);
        })
        .catch(() => {
          /* Background failures intentionally stay silent. */
        })
        .finally(() => {
          if (!active) return;
          setDiscovering(false);
          completedCatalog.current = { baseUrl, apiKey };
        });
    }, 800);
    return () => {
      active = false;
      window.clearTimeout(timer);
      setDiscovering(false);
    };
  }, [baseUrl, apiKey, editingCredentials, configured, setModels]);

  useEffect(() => {
    sequence.current += 1;
    setResults([]);
    setCheckError(null);
    setChecking(false);
    return () => {
      sequence.current += 1;
    };
  }, [baseUrl, apiKey, model, anthropicAuthField]);

  async function check() {
    if (!configured) {
      setCheckError("providerDetection.status.invalid_config");
      return;
    }
    const request = ++sequence.current;
    const start = {
      baseUrl,
      apiKey,
      model,
      anthropicAuthField,
      apiFormat: props.apiFormat,
    };
    setChecking(true);
    setResults([]);
    setCheckError(null);
    const isCurrent = () =>
      request === sequence.current &&
      ["baseUrl", "apiKey", "model", "anthropicAuthField"].every(
        (key) =>
          latest.current[key as keyof Props] ===
          start[key as keyof typeof start],
      );
    try {
      const detected = await detectProviderProtocols({
        baseUrl: baseUrl.trim(),
        apiKey: apiKey.trim(),
        model: model.trim(),
        anthropicAuthField,
      });
      if (!isCurrent()) return;
      setResults(detected);
      const verified = detected.filter(
        (r) => r.text.status === "verified" && r.stream?.status === "verified",
      );
      if (
        latest.current.autoSelectFormat &&
        latest.current.apiFormat === start.apiFormat &&
        !verified.some((r) => r.format === start.apiFormat) &&
        verified[0]
      ) {
        latest.current.onApiFormatChange(verified[0].format);
      }
    } catch {
      if (isCurrent()) setCheckError("providerDetection.failed");
    } finally {
      if (isCurrent()) setChecking(false);
    }
  }

  return (
    <section
      className="space-y-3 rounded-lg border border-border p-4"
      aria-label={t("providerDetection.title")}
    >
      <p className="text-sm font-medium">{t("providerDetection.title")}</p>
      <div
        className="flex items-center gap-2 text-xs text-muted-foreground"
        aria-live="polite"
      >
        {discovering ? (
          <>
            <Loader2 className="h-3 w-3 animate-spin" />
            {t("providerDetection.discovering")}
          </>
        ) : models.length > 0 ? (
          <>
            <span>
              {t("providerDetection.modelsFound", { count: models.length })}
            </span>
            <ModelDropdown models={models} onSelect={props.onModelChange} />
          </>
        ) : (
          <span>{t("providerDetection.manualModelHint")}</span>
        )}
      </div>
      {model && (
        <p className="break-all text-xs text-muted-foreground">
          {t("providerDetection.selectedModel", { model })}
        </p>
      )}
      <p className="text-xs text-muted-foreground">
        {t("providerDetection.costHint")}
      </p>
      <Button
        type="button"
        variant="outline"
        disabled={
          !baseUrl.trim() ||
          !apiKey.trim() ||
          !model.trim() ||
          editingCredentials ||
          checking
        }
        onClick={check}
      >
        {checking && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
        {t(checking ? "providerDetection.checking" : "providerDetection.check")}
      </Button>
      {!model.trim() && (
        <p className="text-xs text-muted-foreground">
          {t("providerDetection.chooseModel")}
        </p>
      )}
      {checkError && (
        <p role="alert" className="text-sm text-destructive">
          {t(checkError)}
        </p>
      )}
      {results.length > 0 && (
        <div className="space-y-2" aria-live="polite">
          {results.map((r) => {
            const verified =
              r.text.status === "verified" && r.stream?.status === "verified";
            const reason =
              r.text.status === "verified" ? r.stream?.status : r.text.status;
            return (
              <div
                key={r.format}
                className="flex flex-wrap items-center justify-between gap-2 text-xs"
              >
                <span>{t(labels[r.format])}</span>
                <span
                  className={
                    verified
                      ? "text-green-600 dark:text-green-400"
                      : "text-muted-foreground"
                  }
                >
                  {verified
                    ? t("providerDetection.verified")
                    : t(
                        `providerDetection.status.${reason ?? "invalid_response"}`,
                      )}
                </span>
                {verified && props.apiFormat !== r.format && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      props.onManualFormatSelect?.();
                      props.onApiFormatChange(r.format);
                    }}
                  >
                    {t("providerDetection.use")}
                  </Button>
                )}
              </div>
            );
          })}
          <p className="text-xs text-muted-foreground">
            {t("providerDetection.scopeHint")}
          </p>
        </div>
      )}
    </section>
  );
}
