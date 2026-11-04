import { useState } from "react";
import { Box } from "lucide-react";
import { ProviderIcon } from "@/components/ProviderIcon";
import { hasIcon } from "@/icons/extracted";
import { globalProviderPresets } from "@/config/globalProviderPresets";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";

export function normalizeUpstreamIcon(value?: string | null) {
  if (!value) return undefined;
  if (hasIcon(value) || value.startsWith("data:image/")) return value;
  const name = value.replace(/-color$/, "");
  const aliases: Record<string, string> = {
    google: "gemini",
    anthropic: "anthropic",
    moonshot: "kimi",
    tencentcloud: "tencent",
    alibabacloud: "alibaba",
  };
  const icon = aliases[name] ?? name;
  return hasIcon(icon) ? icon : undefined;
}

export function upstreamIcon(record: RoutingUpstream) {
  if (record.icon === "letter") return "letter";
  const explicit = normalizeUpstreamIcon(record.icon);
  if (explicit) return explicit;
  if (record.source.kind === "codex") return "openai";
  const saved = globalProviderPresets.find(
    (preset) => preset.id === record.presetId,
  );
  if (saved) return saved.icon;
  let host = "";
  try {
    host = new URL(record.source.baseUrl).host;
  } catch {
    /* Incomplete drafts keep the default icon. */
  }
  return globalProviderPresets.find((preset) =>
    [
      preset.baseUrl,
      ...Object.values(preset.endpoints ?? {}),
      ...Object.values(preset.china?.endpoints ?? {}),
      ...(preset.regions ?? []).flatMap((region) =>
        Object.values(region.endpoints),
      ),
    ].some((url) => new URL(url).host === host),
  )?.icon;
}

export function UpstreamIcon({
  icon,
  name,
  size = 22,
}: {
  icon?: string;
  name: string;
  size?: number;
}) {
  const [failed, setFailed] = useState<string>();
  if (icon?.startsWith("data:image/") && failed !== icon) {
    return (
      <img
        src={icon}
        alt=""
        width={size}
        height={size}
        style={{ width: size, height: size, objectFit: "contain" }}
        onError={() => setFailed(icon)}
      />
    );
  }
  if (icon === "letter")
    return (
      <span
        className="pv-letter-icon"
        style={{ width: size, height: size, fontSize: size * 0.65 }}
        aria-hidden
      >
        {name.trim().slice(0, 2).toLocaleUpperCase()}
      </span>
    );
  const builtin = normalizeUpstreamIcon(icon);
  if (builtin && !builtin.startsWith("data:"))
    return (
      <ProviderIcon
        icon={builtin}
        name={name}
        size={size}
        showFallback={false}
      />
    );
  return <Box size={size} strokeWidth={1.65} aria-hidden />;
}
