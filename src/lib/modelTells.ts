// Mirrors Magpie's modelTells: only equal labels in the same group need a hint.
export function modelTells<T>(
  items: readonly T[],
  label: (item: T) => string,
  id: (item: T) => string,
  key: (item: T) => string = () => "",
): Map<string, string> {
  const by = new Map<string, Set<string>>();
  for (const item of items) {
    const value = id(item);
    const name = label(item);
    if (!value || !name) continue;
    const group = `${key(item)}\0${name}`;
    if (!by.has(group)) by.set(group, new Set());
    by.get(group)!.add(value);
  }
  const out = new Map<string, string>();
  for (const set of by.values()) {
    if (set.size < 2) continue;
    const ids = [...set];
    const segments = ids.map((value) => value.split("/"));
    const first = segments[0];
    let prefix = 0;
    while (
      segments.every(
        (parts) => parts.length > prefix && parts[prefix] === first[prefix],
      )
    )
      prefix++;
    let suffix = 0;
    while (
      segments.every(
        (parts) =>
          parts.length - suffix > prefix &&
          parts[parts.length - 1 - suffix] === first[first.length - 1 - suffix],
      )
    )
      suffix++;
    let tells = segments.map(
      (parts, index) =>
        parts.slice(prefix, parts.length - suffix).join("/") || ids[index],
    );
    if (new Set(tells).size < tells.length) tells = ids;
    ids.forEach((value, index) => out.set(value, tells[index]));
  }
  return out;
}
