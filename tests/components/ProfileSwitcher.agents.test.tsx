import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { APP_IDS } from "@/config/appConfig";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { toast } from "sonner";
import { profilesApi } from "@/lib/api/profiles";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

vi.mock("@/lib/api/profiles", () => ({
  profilesApi: { list: vi.fn(), captureBundle: vi.fn(), undoBundle: vi.fn() },
}));
function mount(app: "gemini" | "kimi" | "qoder" | "trae" | "zcode") {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <ProfileSwitcher activeApp={app} />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  HTMLElement.prototype.scrollIntoView = vi.fn();
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  vi.mocked(profilesApi.list).mockResolvedValue({
    profiles: [],
    currentIds: { claude: null, claudeDesktop: null, codex: null },
    agents: APP_IDS.map((app) => ({
      app,
      available: app !== "doubaowork",
      provider: app !== "doubaowork",
      mcp: false,
      skills: false,
      instructions: false,
      reason: app === "doubaowork" ? "profiles.bundle.nativeLoginOnly" : null,
    })),
  });
});
describe("Profile entry for added agents", () => {
  it.each(["gemini", "kimi", "qoder", "trae", "zcode"] as const)(
    "offers cross-agent profiles from %s",
    async (app) => {
      mount(app);
      fireEvent.click(await screen.findByRole("combobox"));
      fireEvent.click(await screen.findByText("新建配置方案"));
      expect(screen.getByRole("checkbox", { name: /Gemini/ })).toBeEnabled();
      expect(screen.getByLabelText("Qoder CN", { exact: false })).toBeEnabled();
      expect(screen.getByLabelText("TRAE CN", { exact: false })).toBeEnabled();
      expect(
        screen.getByText(zh.profiles.bundle.nativeLoginOnly),
      ).toBeInTheDocument();
    },
  );
});

it.each([false, true])(
  "handles menu restoration with an external conflict=%s",
  async (conflict) => {
    const data = await profilesApi.list();
    vi.mocked(profilesApi.list).mockResolvedValue({
      ...data,
      undo: {
        id: "undo-1",
        profileId: "profile-1",
        name: "Development",
        apps: ["claude", "kimi"],
      },
    });
    if (conflict)
      vi.mocked(profilesApi.undoBundle).mockRejectedValue(
        new Error("profiles.bundle.conflict"),
      );
    else vi.mocked(profilesApi.undoBundle).mockResolvedValue(undefined);
    mount("kimi");
    fireEvent.click(await screen.findByRole("combobox"));
    fireEvent.change(
      screen.getByPlaceholderText(zh.profiles.searchPlaceholder),
      {
        target: { value: zh.profiles.bundle.undoLast },
      },
    );
    await waitFor(() =>
      expect(screen.getByText(zh.profiles.bundle.undoLast)).toBeVisible(),
    );
    fireEvent.click(await screen.findByText(zh.profiles.bundle.undoLast));
    fireEvent.click(
      await screen.findByRole("button", { name: zh.common.confirm }),
    );
    await waitFor(() =>
      expect(profilesApi.undoBundle).toHaveBeenCalledWith("undo-1"),
    );
    await waitFor(() => {
      if (conflict)
        expect(toast.error).toHaveBeenCalledWith(zh.profiles.bundle.conflict);
      else
        expect(toast.success).toHaveBeenCalledWith(zh.profiles.bundle.restored);
    });
  },
);
