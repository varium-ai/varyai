import { afterEach, expect, it, vi } from "vitest";
import {
  fetchReleaseNotes,
  releaseNotesPageUrl,
  meaningfulReleaseNotes,
} from "@/lib/releaseNotes";

afterEach(() => vi.unstubAllGlobals());
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
