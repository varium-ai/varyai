export function isImageGenerationModel(value: unknown): boolean {
  const row =
    value && typeof value === "object"
      ? (value as Record<string, unknown>)
      : {};
  const prefs = row.modelPreferences as Record<string, unknown> | undefined;
  const kind = prefs?.kind ?? row.kind;
  if (typeof kind === "string" && kind) return kind === "image";
  const output = row.outputModalities ?? row.output_modalities;
  if (Array.isArray(output) && output.includes("image")) return true;
  const id = String(
    typeof value === "string" ? value : (row.model ?? row.id ?? ""),
  ).toLowerCase();
  return (
    /gpt-image|chatgpt-image|dall-e|(^|[/_-])imagen([/_.-]|$)|(^|[/_-])flux([/_.-]|$)|stable-diffusion|(^|[/_-])sdxl([/_.-]|$)/.test(
      id,
    ) ||
    (id.includes("gemini") && /(^|[-_/])image([-_/]|$)/.test(id))
  );
}
