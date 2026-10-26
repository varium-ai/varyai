import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { profilesApi } from "@/lib/api";
import { useApplyProfileMutation } from "./profiles";
import type { ProfileScope } from "@/lib/api/profiles";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { app?: string }) =>
      options?.app ? `${key}: ${options.app}` : key,
  }),
}));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn() },
}));
vi.mock("@/lib/api", () => ({
  profilesApi: { apply: vi.fn() },
  providersApi: { updateTrayMenu: vi.fn().mockResolvedValue(undefined) },
}));

beforeEach(() => vi.clearAllMocks());

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
