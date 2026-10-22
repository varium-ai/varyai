import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ProfileManageDialog } from "@/components/profiles/ProfileManageDialog";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { profilesApi } from "@/lib/api/profiles";
import {
  useProfilesQuery,
  useUpdateProfileMutation,
} from "@/lib/query/profiles";

vi.mock("@/lib/query/profiles", () => ({
  useProfilesQuery: vi.fn(),
  useUpdateProfileMutation: vi.fn(),
  useDeleteProfileMutation: () => ({ mutate: vi.fn() }),
}));
vi.mock("@/lib/api/profiles", () => ({
  profilesApi: { captureBundle: vi.fn() },
}));

beforeEach(() => {
  vi.clearAllMocks();
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  vi.mocked(useProfilesQuery).mockReturnValue({
    data: {
      profiles: [
        {
          id: "work",
          name: "工作",
          payload: {
            bundle: {
              selections: [{ app: "claude" }, { app: "codex" }],
            },
          },
        },
      ],
    },
  } as ReturnType<typeof useProfilesQuery>);
  vi.mocked(useUpdateProfileMutation).mockReturnValue({
    mutate: vi.fn(),
    isPending: false,
  } as unknown as ReturnType<typeof useUpdateProfileMutation>);
});

it("explains explicit snapshot updates and edits a named bundle without applying it", async () => {
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  render(
    <QueryClientProvider client={client}>
      <ProfileManageDialog isOpen onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  expect(screen.getByText(/多 Agent 方案不会自动保存/)).toBeInTheDocument();
  fireEvent.click(screen.getByTitle("重命名"));
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "个人" } });
  fireEvent.click(screen.getByTitle("确定"));
  expect(
    vi.mocked(useUpdateProfileMutation).mock.results[0].value.mutate,
  ).toHaveBeenCalledWith({ id: "work", name: "个人" }, expect.anything());
  fireEvent.click(screen.getByTitle("取消"));
  fireEvent.click(screen.getByTitle("用当前配置更新快照"));
  await waitFor(() =>
    expect(profilesApi.captureBundle).toHaveBeenCalledWith(
      "工作",
      ["claude", "codex"],
      "work",
    ),
  );
  await waitFor(() =>
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["profiles"] }),
  );
});
