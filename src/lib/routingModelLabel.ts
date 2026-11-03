import type { AgentModel } from "@/lib/api/agentConnections";
import type { RoutingTrace } from "@/lib/api/routingTrace";

type RequestedModel = Pick<RoutingTrace, "model" | "groupName" | "candidates">;

export function routingModelLabel(
  row: RequestedModel,
  catalog: readonly AgentModel[],
): string | undefined {
  if (!/^vary-(?:auto|provider)\//.test(row.model)) return row.model;
  const direct = /^vary-(?:auto|provider)\/model-[a-f0-9]{64}$/i.test(
    row.model,
  );
  const alias = direct
    ? row.model.replace(/^vary-auto\//, "vary-provider/")
    : row.model;
  const listed = catalog.find((model) => model.id === alias);

  if (!direct || listed?.group === "routing") {
    return row.groupName || listed?.name || undefined;
  }
  if (listed?.name) {
    // The shared picker appends the provider; the trace already shows it separately.
    const provider = listed.group || row.candidates[0]?.providerName;
    const suffix = provider ? ` \u00b7 ${provider}` : "";
    return suffix && listed.name.endsWith(suffix)
      ? listed.name.slice(0, -suffix.length)
      : listed.name;
  }
  const models = [
    ...new Set(
      row.candidates
        .map((candidate) => candidate.model)
        .filter((model) => model && !/^vary-(?:auto|provider)\//.test(model)),
    ),
  ];
  // Account pools share one wire model. Never label a group by an arbitrary fallback.
  return models.length === 1 ? models[0] : undefined;
}
