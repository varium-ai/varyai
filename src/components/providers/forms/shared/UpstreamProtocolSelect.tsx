import { useTranslation } from "react-i18next";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ClaudeApiFormat } from "@/types";

export const UPSTREAM_FORMATS: ClaudeApiFormat[] = [
  "anthropic",
  "openai_chat",
  "openai_responses",
  "gemini_native",
];
const labels = [
  "apiFormatAnthropic",
  "apiFormatOpenAIChat",
  "apiFormatOpenAIResponses",
  "apiFormatGeminiNative",
];

export function UpstreamProtocolSelect({
  value,
  onChange,
}: {
  value: ClaudeApiFormat;
  onChange: (value: ClaudeApiFormat) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-2">
      <Label htmlFor="upstream-api-format">{t("providerForm.apiFormat")}</Label>
      <Select
        value={value}
        onValueChange={(value) => onChange(value as ClaudeApiFormat)}
      >
        <SelectTrigger
          id="upstream-api-format"
          className="h-11 rounded-xl px-4 text-sm"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent className="rounded-xl">
          {UPSTREAM_FORMATS.map((format, index) => (
            <SelectItem
              key={format}
              value={format}
              className="min-h-11 rounded-lg pr-4"
            >
              {t(`providerForm.${labels[index]}`)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <p className="text-xs text-muted-foreground">
        {t("providerForm.protocolBridgeHint")}
      </p>
    </div>
  );
}

export function nativeUpstreamFormat(protocol: unknown): ClaudeApiFormat {
  return protocol === "anthropic"
    ? "anthropic"
    : protocol === "openai-responses"
      ? "openai_responses"
      : "openai_chat";
}
