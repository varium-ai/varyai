// Call-kind aliases from the reference implementation's usage purpose catalog (MIT).
// See third_party/routing-reference/LICENSE. Recorded kinds remain unchanged.
const aliases: Record<string, string> = {};
for (const kinds of [
  ["guardian", "auto_review", "guardian_review"],
  ["review"],
  ["compact"],
  ["memory_consolidation", "memgen", "memory"],
  ["thread_title", "thread_title_reconsideration", "title_generation", "title"],
  ["thread_description"],
  ["composer_predictions"],
  ["collab_spawn", "thread_spawn", "agent_job"],
  ["luna_reserve"],
  ["ambient_suggestions", "ambient_suggestion_safety"],
  ["web_search"],
  ["vision"],
  ["routing"],
  ["provider_test"],
  ["api_detect"],
  ["model_list"],
  ["mcp"],
  ["unknown"],
]) {
  for (const kind of kinds) aliases[kind] = kinds[0];
}

export function routingPurpose(kind?: string | null): string {
  if (!kind) return "unmarked";
  return `kind:${Object.hasOwn(aliases, kind) ? aliases[kind] : kind}`;
}

export function routingPurposeTranslation(purpose: string): string | null {
  if (purpose === "unmarked") return "routingPage.purposes.unmarked";
  const kind = purpose.slice(5);
  return Object.hasOwn(aliases, kind)
    ? `routingPage.purposes.${aliases[kind]}`
    : null;
}
