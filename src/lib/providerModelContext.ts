type Row = Record<string, unknown>;
const object = (value: unknown): Row =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Row)
    : {};
export function modelContextWindow(value: unknown): number | undefined {
  const row = object(value);
  for (const field of [
    "contextWindow",
    "context_window",
    "context_length",
    "inputTokenLimit",
  ]) {
    const n = Number(row[field]);
    if (Number.isSafeInteger(n) && n >= 2048) return n;
  }
}
export function mergeModelMetadata(previous: unknown, next: unknown): Row {
  const row = { ...object(previous), ...object(next) };
  const window = modelContextWindow(next) ?? modelContextWindow(previous);
  if (window !== undefined) row.contextWindow = window;
  return row;
}
export function modelContextBadge(
  window: number | undefined,
  name: string,
): string {
  if (!window || window < 1e6 || /\b\d+M\b/.test(name)) return "";
  const m = window / 1e6;
  return `${Number.isInteger(m) ? m : m.toFixed(1)}M`;
}
