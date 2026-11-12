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
