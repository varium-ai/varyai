import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
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
  fireEvent.click(
    screen.getByRole("button", { name: zh.profiles.bundle.recapture }),
  );
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

it("shows progress immediately and prevents duplicate snapshot refreshes", async () => {
  let finish!: () => void;
  vi.mocked(profilesApi.captureBundle).mockReturnValue(
    new Promise((resolve) => {
      finish = () => resolve({} as never);
    }),
  );
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProfileManageDialog isOpen onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  const refresh = screen.getByRole("button", {
    name: zh.profiles.bundle.recapture,
  });
  fireEvent.click(refresh);
  expect(refresh).toHaveAttribute("aria-busy", "true");
  expect(refresh).toBeDisabled();
  fireEvent.click(refresh);
  expect(profilesApi.captureBundle).toHaveBeenCalledOnce();
  await act(async () => finish());
  await waitFor(() => expect(refresh).toBeEnabled());
  expect(refresh).toHaveAttribute("aria-busy", "false");
});

it("keeps a long profile name identifiable and allows cancelling deletion", async () => {
  const name = "LongProfileName".repeat(100);
  const data = vi.mocked(useProfilesQuery)().data!;
  vi.mocked(useProfilesQuery).mockReturnValue({
    data: { ...data, profiles: [{ ...data.profiles[0], name }] },
  } as ReturnType<typeof useProfilesQuery>);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProfileManageDialog isOpen onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  expect(screen.getByTitle(name)).toBeInTheDocument();
  fireEvent.click(screen.getByTitle("删除"));
  expect(
    await screen.findByRole("dialog", { name: zh.profiles.deleteConfirmTitle }),
  ).toHaveTextContent(name);
  fireEvent.click(screen.getByRole("button", { name: zh.common.cancel }));
  expect(
    screen.queryByRole("dialog", { name: zh.profiles.deleteConfirmTitle }),
  ).toBeNull();
});

it("shows the snapshot refresh hint on hover without the native tooltip delay", async () => {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProfileManageDialog isOpen onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  const refresh = screen.getByRole("button", {
    name: zh.profiles.bundle.recapture,
  });
  fireEvent.pointerMove(refresh, { pointerType: "mouse" });
  expect(
    await screen.findByRole("tooltip", {}, { timeout: 200 }),
  ).toHaveTextContent(zh.profiles.bundle.recapture);
  expect(profilesApi.captureBundle).not.toHaveBeenCalled();
});
