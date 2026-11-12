import { afterEach, expect, it, vi } from "vitest";
import {
  fetchReleaseNotes,
  releaseNotesPageUrl,
  meaningfulReleaseNotes,
  localizedReleaseNotes,
  fetchReleaseHistory,
} from "@/lib/releaseNotes";

afterEach(() => vi.unstubAllGlobals());

function publicRelease(version: string, extra = {}) {
  return {
    tag_name: `v${version}`,
    body: `## Fixes\n- Change ${version}.`,
    ...extra,
  };
}

it("loads all published versions between the installed and target versions in descending version order", async () => {
  const request = vi.fn().mockResolvedValue({
    ok: true,
    json: async () => [
      publicRelease("0.3.2"),
      publicRelease("0.3.4"),
      publicRelease("0.3.3"),
      publicRelease("0.3.0"),
      publicRelease("0.3.5"),
      publicRelease("0.3.1", { draft: true }),
      publicRelease("0.3.4-beta.1", { prerelease: true }),
      publicRelease("0.3.3"),
      { tag_name: "invalid", body: "Ignore." },
    ],
  });
  vi.stubGlobal("fetch", request);
  expect(await fetchReleaseHistory("0.3.0", "0.3.4")).toEqual({
    releases: ["0.3.4", "0.3.3", "0.3.2"].map((version) => ({
      version,
      notes: `## Fixes\n- Change ${version}.`,
    })),
    complete: true,
  });
  expect(request).toHaveBeenCalledWith(
    "https://api.github.com/repos/varium-ai/varyai/releases?per_page=100&page=1",
    expect.objectContaining({
      credentials: "omit",
      signal: expect.any(AbortSignal),
    }),
  );
});

it("compares numeric versions and prerelease identifiers rather than strings", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue({
      ok: true,
      json: async () => [
        publicRelease("0.3.9"),
        publicRelease("0.3.10"),
        publicRelease("0.3.11-beta.2", { prerelease: true }),
        publicRelease("0.3.11-beta.10", { prerelease: true }),
        publicRelease("0.3.11"),
      ],
    }),
  );
  const history = await fetchReleaseHistory("0.3.8", "0.3.11-beta.10");
  expect(history.releases.map((release) => release.version)).toEqual([
    "0.3.11-beta.10",
    "0.3.11-beta.2",
    "0.3.10",
    "0.3.9",
  ]);
});

it("continues to the next page and reports incomplete history if that page fails", async () => {
  const page = Array.from({ length: 100 }, (_, index) =>
    publicRelease(`0.1.${index}`),
  );
  const request = vi
    .fn()
    .mockResolvedValueOnce({ ok: true, json: async () => page })
    .mockRejectedValueOnce(new Error("synthetic-private-detail"));
  vi.stubGlobal("fetch", request);
  const history = await fetchReleaseHistory("0.1.97", "0.2.0");
  expect(history).toEqual({
    releases: [
      { version: "0.1.99", notes: "## Fixes\n- Change 0.1.99." },
      { version: "0.1.98", notes: "## Fixes\n- Change 0.1.98." },
    ],
    complete: false,
  });
  expect(request).toHaveBeenCalledTimes(2);
});

it("bounds long history to the latest 40 versions and reports the omission", async () => {
  const request = vi.fn().mockResolvedValue({
    ok: true,
    json: async () =>
      Array.from({ length: 100 }, (_, index) => publicRelease(`0.1.${index}`)),
  });
  vi.stubGlobal("fetch", request);
  const history = await fetchReleaseHistory("0.1.0", "0.2.0");
  expect(history.releases).toHaveLength(40);
  expect(history.releases[0].version).toBe("0.1.99");
  expect(history.complete).toBe(false);
  expect(request.mock.calls.length).toBeLessThanOrEqual(5);
});

it("does not request history without a valid increasing version range", async () => {
  const request = vi.fn();
  vi.stubGlobal("fetch", request);
  expect((await fetchReleaseHistory("", "0.3.4")).complete).toBe(false);
  expect((await fetchReleaseHistory("0.3.4", "0.3.4")).releases).toEqual([]);
  expect(request).not.toHaveBeenCalled();
});

it("cancels history requests through the caller's signal", async () => {
  const controller = new AbortController();
  controller.abort();
  const request = vi.fn();
  vi.stubGlobal("fetch", request);
  await fetchReleaseHistory("0.3.0", "0.3.4", controller.signal);
  expect(request).not.toHaveBeenCalled();
});

it("times out optional history without surfacing request details", async () => {
  vi.useFakeTimers();
  try {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockImplementation(
          (_url, { signal }) =>
            new Promise((_resolve, reject) =>
              signal.addEventListener("abort", () =>
                reject(new Error("synthetic-private-detail")),
              ),
            ),
        ),
    );
    const history = fetchReleaseHistory("0.3.0", "0.3.4");
    await vi.advanceTimersByTimeAsync(8000);
    expect(await history).toEqual({ releases: [], complete: false });
  } finally {
    vi.useRealTimers();
  }
});

const multilingualNotes = `## Features
- English change.

<!-- lang:zh -->
## 新增功能
- 简体变更。

<!-- lang:ja -->
## 新機能
- 日本語の変更。

<!-- lang:zh-TW -->
## 新增功能
- 繁體變更。`;

it.each([
  ["en", "## Features\n- English change."],
  ["en-US", "## Features\n- English change."],
  ["zh-CN", "## 新增功能\n- 简体变更。"],
  ["ja-JP", "## 新機能\n- 日本語の変更。"],
  ["zh-TW", "## 新增功能\n- 繁體變更。"],
])("selects only the %s release notes", (language, expected) => {
  expect(localizedReleaseNotes(multilingualNotes, language)).toBe(expected);
});

it("falls back to English when a translation is absent", () => {
  const notes =
    "## Features\n- English.\n<!-- lang:zh -->\n## 新功能\n- 中文。";
  expect(localizedReleaseNotes(notes, "ja")).toBe("## Features\n- English.");
  expect(localizedReleaseNotes(notes, "zh-TW")).toBe("## 新功能\n- 中文。");
  expect(localizedReleaseNotes(notes, "fr")).toBe("## Features\n- English.");
});

it("supports explicit English markers and ignores empty translations", () => {
  const notes =
    "<!-- lang:en -->\nEnglish.\n<!-- lang:ja -->\n\n<!-- lang:zh -->\n中文。";
  expect(localizedReleaseNotes(notes, "ja")).toBe("English.");
  expect(localizedReleaseNotes("## Fixes\n- One fix.", "zh")).toBe(
    "## Fixes\n- One fix.",
  );
});
it("uses the exact available version for the public release notes request", async () => {
  const request = vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({
      tag_name: "v0.3.0",
      body: "## Fixes\n- Keep accounts.",
    }),
  });
  vi.stubGlobal("fetch", request);
  expect(await fetchReleaseNotes("0.3.0")).toBe("## Fixes\n- Keep accounts.");
  expect(request).toHaveBeenCalledWith(
    "https://api.github.com/repos/varium-ai/varyai/releases/tags/v0.3.0",
    expect.objectContaining({
      credentials: "omit",
      signal: expect.any(AbortSignal),
    }),
  );
  expect(releaseNotesPageUrl("0.3.0")).toBe(
    "https://github.com/varium-ai/varyai/releases/tag/v0.3.0",
  );
});
it("does not substitute notes from another release", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ tag_name: "v9.0.0", body: "Unrelated release" }),
    }),
  );
  expect(await fetchReleaseNotes("0.3.0")).toBe("");
});
it("rejects failed requests without echoing their response details", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue({
      ok: false,
      text: async () => "synthetic-private-response",
    }),
  );
  await expect(fetchReleaseNotes("0.3.0")).rejects.toThrow(
    "Could not fetch release notes.",
  );
});
it("treats missing notes and the old release-page placeholder as empty", () => {
  expect(meaningfulReleaseNotes(undefined)).toBe("");
  expect(
    meaningfulReleaseNotes(" See the GitHub release page for release notes.\n"),
  ).toBe("");
  expect(meaningfulReleaseNotes("## Fixes\n- Keep accounts.")).toBe(
    "## Fixes\n- Keep accounts.",
  );
});
it("does not request release data for an invalid version", async () => {
  const request = vi.fn();
  vi.stubGlobal("fetch", request);
  expect(await fetchReleaseNotes("../../private")).toBe("");
  expect(request).not.toHaveBeenCalled();
});
