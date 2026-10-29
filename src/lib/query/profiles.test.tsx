import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { profilesApi } from "@/lib/api";
import {
  useApplyProfileMutation,
  useCreateProfileMutation,
  useUpdateProfileMutation,
} from "./profiles";
import type { ProfileScope } from "@/lib/api/profiles";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { app?: string; detail?: string }) =>
      options?.app || options?.detail
        ? `${key}: ${options.app ?? options.detail}`
        : key,
  }),
}));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn() },
}));
vi.mock("@/lib/api", () => ({
  profilesApi: { apply: vi.fn(), create: vi.fn(), update: vi.fn() },
  providersApi: { updateTrayMenu: vi.fn().mockResolvedValue(undefined) },
}));

beforeEach(() => vi.clearAllMocks());

it.each(["create", "update"] as const)(
  "translates a backend duplicate-name error during %s",
  async (operation) => {
    vi.mocked(profilesApi[operation]).mockRejectedValue(
      new Error("profiles.nameExists"),
    );
    const { result } = renderHook(
      () =>
        operation === "create"
          ? useCreateProfileMutation()
          : useUpdateProfileMutation(),
      {
        wrapper: ({ children }) => (
          <QueryClientProvider client={new QueryClient()}>
            {children}
          </QueryClientProvider>
        ),
      },
    );
    await act(async () => {
      await expect(
        result.current.mutateAsync({
          id: "fixture",
          name: "Work",
          scope: "claude",
        }),
      ).rejects.toThrow("profiles.nameExists");
    });
    expect(toast.error).toHaveBeenCalledWith(
      `profiles.${operation}Failed: profiles.nameExists`,
      { closeButton: true },
    );
  },
);

it.each<ProfileScope>(["claude", "claude-desktop", "codex"])(
  "identifies the restart needed after applying a legacy %s profile",
  async (scope) => {
    vi.mocked(profilesApi.apply).mockResolvedValue([]);
    const client = new QueryClient();
    const { result } = renderHook(() => useApplyProfileMutation(), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    });
    await act(() =>
      result.current.mutateAsync({ id: "synthetic-profile", scope }),
    );
    expect(toast.success).toHaveBeenCalledWith(
      "profiles.applySuccess",
      expect.objectContaining({
        description: `profiles.bundle.${scope === "claude" ? "restartSession" : "restartClient"}: apps.${scope}`,
      }),
    );
  },
);
