import {
  getUsageRoutingModelId,
  normalizeUsageModelId,
  type ModelStats,
  type UsageModelLabel,
} from "@/types/usage";

const isRoutingId = (value: string) => getUsageRoutingModelId(value) != null;

export function isReadableUsageModelName(value: string): boolean {
  return (
    !!value.trim() &&
    !["unknown", "-codex"].includes(value.trim().toLowerCase()) &&
    !isRoutingId(value)
  );
}

export interface UsageModelOption {
  value: string;
  routingModels?: string[];
  routingId?: string;
  resolvedLabel?: UsageModelLabel;
}

export function getUsageModelOptions(
  stats: ModelStats[],
  selectedModel?: string,
  labels: UsageModelLabel[] = [],
): UsageModelOption[] {
  const options = new Map<string, Set<string>>();
  for (const stat of stats) {
    const billingModel = normalizeUsageModelId(stat.model);
    const names = [
      billingModel,
      ...(stat.aliases ?? []).map(normalizeUsageModelId),
    ];
    // Prefer the billing model; old records may only have a readable alias.
    const readableModels = isReadableUsageModelName(billingModel)
      ? [billingModel]
      : names.filter(isReadableUsageModelName);
    for (const name of names) {
      if (!options.has(name)) options.set(name, new Set());
      if (isRoutingId(name)) {
        for (const model of readableModels) options.get(name)?.add(model);
      }
    }
  }
  // Keep the original filter even when it no longer occurs in the current range.
  if (selectedModel) {
    const selected = normalizeUsageModelId(selectedModel);
    if (!options.has(selected)) options.set(selected, new Set());
  }
  const labelMap = new Map(labels.map((label) => [label.id, label]));
  return Array.from(options, ([value, models]) => ({
    value,
    routingModels: isRoutingId(value) ? Array.from(models).sort() : undefined,
    routingId: getUsageRoutingModelId(value),
    resolvedLabel: labelMap.get(value),
  }));
}

export function getUsageModelDisplayName(
  value: string,
  names: Readonly<Record<string, string>>,
  unknown: (id: string) => string,
): string {
  const id = getUsageRoutingModelId(value);
  return id ? (names[id] ?? unknown(id.slice(6, 18))) : value;
}
