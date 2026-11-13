import {
  RELEASES_API_URL,
  RELEASES_LATEST_URL,
  RELEASES_URL,
} from "@/config/brand";

const legacyPlaceholder = "See the GitHub release page for release notes.";

export function meaningfulReleaseNotes(notes?: string): string {
  const text = notes?.trim() ?? "";
  return text === legacyPlaceholder ? "" : text;
}

export function localizedReleaseNotes(notes: string, language: string): string {
  // Unmarked release descriptions use English before the first language marker.
  const sections = new Map<string, string>();
  const parts = notes.split(/<!--\s*lang:([a-z0-9-]+)\s*-->/gi);
  sections.set("en", parts[0].trim());
  for (let index = 1; index < parts.length; index += 2) {
    const locale = parts[index].toLowerCase();
    const content = parts[index + 1].trim();
    if (content) {
      sections.set(
        locale,
        [sections.get(locale), content].filter(Boolean).join("\n\n"),
      );
    }
  }
  const locale = language.toLowerCase();
  const base = locale.split("-")[0];
  return sections.get(locale) || sections.get(base) || sections.get("en") || "";
}

function releaseTag(version: string): string | undefined {
  const normalized = version.replace(/^v/, "");
  return /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(
    normalized,
  )
    ? `v${normalized}`
    : undefined;
}

export function releaseNotesPageUrl(version: string): string {
  const tag = releaseTag(version);
  return tag
    ? `${RELEASES_URL}/tag/${encodeURIComponent(tag)}`
    : RELEASES_LATEST_URL;
}

export interface VersionReleaseNotes {
  version: string;
  notes: string;
}

export interface ReleaseHistory {
  releases: VersionReleaseNotes[];
  complete: boolean;
}

function versionParts(version: string): string[] {
  const [core, ...prerelease] = version
    .replace(/^v/, "")
    .split("+")[0]
    .split("-");
  return [
    ...core.split("."),
    ...(prerelease.length ? prerelease.join("-").split(".") : []),
  ];
}

function compareVersions(left: string, right: string): number {
  const a = versionParts(left);
  const b = versionParts(right);
  for (let index = 0; index < Math.max(a.length, b.length); index++) {
    const x = a[index];
    const y = b[index];
    if (x === y) continue;
    if (x === undefined) return index === 3 ? 1 : -1;
    if (y === undefined) return index === 3 ? -1 : 1;
    const numericX = /^\d+$/.test(x);
    const numericY = /^\d+$/.test(y);
    if (numericX && numericY) {
      if (BigInt(x) !== BigInt(y)) return BigInt(x) < BigInt(y) ? -1 : 1;
    } else if (numericX !== numericY) {
      return numericX ? -1 : 1;
    } else {
      return x < y ? -1 : 1;
    }
  }
  return 0;
}

export async function fetchReleaseHistory(
  currentVersion: string,
  targetVersion: string,
  signal?: AbortSignal,
): Promise<ReleaseHistory> {
  if (!releaseTag(currentVersion) || !releaseTag(targetVersion)) {
    return { releases: [], complete: false };
  }
  if (compareVersions(currentVersion, targetVersion) >= 0) {
    return { releases: [], complete: true };
  }
  const releases = new Map<string, VersionReleaseNotes>();
  const controller = new AbortController();
  const abort = () => controller.abort();
  if (signal?.aborted) controller.abort();
  else signal?.addEventListener("abort", abort, { once: true });
  const timer = setTimeout(abort, 8000);
  let complete = false;
  try {
    // Bound public history requests by time and page count, regardless of tags.
    for (let page = 1; page <= 5 && !controller.signal.aborted; page++) {
      const response = await fetch(
        `${RELEASES_API_URL}?per_page=100&page=${page}`,
        {
          signal: controller.signal,
          headers: { Accept: "application/vnd.github+json" },
          credentials: "omit",
          referrerPolicy: "no-referrer",
        },
      );
      if (!response.ok) break;
      const data: unknown = await response.json();
      if (!Array.isArray(data)) break;
      for (const entry of data) {
        if (!entry || typeof entry !== "object") continue;
        const { tag_name: tag, body, draft, prerelease } = entry;
        if (typeof tag !== "string" || !releaseTag(tag) || draft) continue;
        const version = tag.replace(/^v/, "");
        if (
          (versionParts(targetVersion).length === 3 &&
            (prerelease || versionParts(version).length > 3)) ||
          compareVersions(version, currentVersion) <= 0 ||
          compareVersions(version, targetVersion) > 0
        )
          continue;
        releases.set(version.split("+")[0], {
          version,
          notes: meaningfulReleaseNotes(
            typeof body === "string" ? body : undefined,
          ),
        });
      }
      if (data.length < 100) {
        complete = true;
        break;
      }
    }
  } catch {
    // Optional history must not block installing the target release.
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
  }
  const sorted = [...releases.values()].sort((a, b) =>
    compareVersions(b.version, a.version),
  );
  return {
    releases: sorted.slice(0, 40),
    complete: complete && sorted.length <= 40,
  };
}

export async function fetchReleaseNotes(
  version: string,
  signal?: AbortSignal,
): Promise<string> {
  const tag = releaseTag(version);
  if (!tag) return "";
  const controller = new AbortController();
  const abort = () => controller.abort();
  if (signal?.aborted) controller.abort();
  else signal?.addEventListener("abort", abort, { once: true });
  const timer = setTimeout(abort, 8000);
  try {
    const response = await fetch(
      `${RELEASES_API_URL}/tags/${encodeURIComponent(tag)}`,
      {
        signal: controller.signal,
        headers: { Accept: "application/vnd.github+json" },
        credentials: "omit",
        referrerPolicy: "no-referrer",
      },
    );
    if (!response.ok) throw new Error("Could not fetch release notes.");
    const release: unknown = await response.json();
    if (!release || typeof release !== "object") return "";
    const { tag_name: name, body } = release as Record<string, unknown>;
    return name === tag && typeof body === "string"
      ? meaningfulReleaseNotes(body)
      : "";
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", abort);
  }
}
