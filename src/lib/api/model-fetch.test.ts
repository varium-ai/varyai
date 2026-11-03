import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TFunction } from "i18next";
import { toast } from "sonner";
import { showFetchModelsError } from "./model-fetch";

vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
const translate = vi.fn((key: string) => key);
const t = translate as unknown as TFunction;

beforeEach(() => vi.clearAllMocks());

describe("model discovery failure feedback", () => {
  it.each([
    [
      "HTTP 429 Too Many Requests: synthetic-private-detail",
      "providerForm.fetchModelsRateLimited",
    ],
    [
      "Request failed: synthetic-private-detail",
      "providerForm.fetchModelsNetworkFailed",
    ],
    ["Invalid models endpoint", "providerForm.fetchModelsInvalidConfig"],
    [
      "Cannot derive models endpoint from full URL",
      "providerForm.fetchModelsInvalidConfig",
    ],
  ])("classifies %s without exposing backend details", (error, key) => {
    showFetchModelsError(error, t);
    expect(toast.error).toHaveBeenCalledWith(key);
    expect(JSON.stringify(translate.mock.calls)).not.toContain(
      "synthetic-private-detail",
    );
  });

  it("reports only the numeric upstream status", () => {
    showFetchModelsError(
      new Error("HTTP 503 Service Unavailable: synthetic-private-detail"),
      t,
    );
    expect(translate).toHaveBeenCalledWith(
      "providerForm.fetchModelsUpstreamFailed",
      { status: 503 },
    );
    expect(toast.error).toHaveBeenCalledWith(
      "providerForm.fetchModelsUpstreamFailed",
    );
  });

  it.each([
    ["HTTP 401 Unauthorized", "providerForm.fetchModelsAuthFailed"],
    [
      "All candidates failed: HTTP 404",
      "providerForm.fetchModelsEndpointNotFound",
    ],
    ["Request failed: operation timed out", "providerForm.fetchModelsTimeout"],
    ["Failed to parse response", "providerForm.fetchModelsNotSupported"],
    ["synthetic-private-detail", "providerForm.fetchModelsFailed"],
  ])("preserves the existing classification for %s", (error, key) => {
    showFetchModelsError(error, t);
    expect(toast.error).toHaveBeenCalledWith(key);
  });
});
