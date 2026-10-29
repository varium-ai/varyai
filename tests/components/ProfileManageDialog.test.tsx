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
  profilesApi: { captureBundle: vi.fn(), list: vi.fn() },
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(profilesApi.captureBundle).mockResolvedValue({} as never);
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
      agents: ["claude", "codex", "kimi"].map((app) => ({
        app,
        available: true,
        reason: null,
      })),
    },
  } as ReturnType<typeof useProfilesQuery>);
  vi.mocked(profilesApi.list).mockResolvedValue(
    vi.mocked(useProfilesQuery)().data!,
  );
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
      <ProfileManageDialog
        isOpen
        onClose={vi.fn()}
        selectedApps={["claude", "codex", "kimi"]}
      />
    </QueryClientProvider>,
  );
  expect(screen.getByText(/新建方案不会自动保存/)).toBeInTheDocument();
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
      ["claude", "codex", "kimi"],
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
      <ProfileManageDialog
        isOpen
        onClose={vi.fn()}
        selectedApps={["claude", "codex", "kimi"]}
      />
    </QueryClientProvider>,
  );
  const refresh = screen.getByRole("button", {
    name: zh.profiles.bundle.recapture,
  });
  fireEvent.click(refresh);
  expect(refresh).toHaveAttribute("aria-busy", "true");
  expect(refresh).toBeDisabled();
  fireEvent.click(refresh);
  await waitFor(() => expect(profilesApi.captureBundle).toHaveBeenCalledOnce());
  await act(async () => finish());
  await waitFor(() => expect(refresh).toBeEnabled());
  expect(refresh).toHaveAttribute("aria-busy", "false");
});

it("replaces the old snapshot scope with all currently selected configured agents", async () => {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProfileManageDialog
        isOpen
        onClose={vi.fn()}
        selectedApps={["codex", "kimi"]}
      />
    </QueryClientProvider>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: zh.profiles.bundle.recapture }),
  );
  await waitFor(() =>
    expect(profilesApi.captureBundle).toHaveBeenCalledWith(
      "工作",
      ["codex", "kimi"],
      "work",
    ),
  );
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
});

it("preserves the snapshot if the current agent state cannot be read", async () => {
  vi.mocked(profilesApi.list).mockRejectedValue(
    new Error("Synthetic read failure"),
  );
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProfileManageDialog
        isOpen
        onClose={vi.fn()}
        selectedApps={["claude", "kimi"]}
      />
    </QueryClientProvider>,
  );
  const refresh = screen.getByRole("button", {
    name: zh.profiles.bundle.recapture,
  });
  fireEvent.click(refresh);
  await waitFor(() => expect(refresh).toBeEnabled());
  expect(profilesApi.captureBundle).not.toHaveBeenCalled();
});

it("keeps the rename editor open and explains duplicate profile names", () => {
  const data = vi.mocked(useProfilesQuery)().data!;
  vi.mocked(useProfilesQuery).mockReturnValue({
    data: {
      ...data,
      profiles: [
        data.profiles[0],
        { ...data.profiles[0], id: "other", name: "Personal" },
      ],
    },
  } as ReturnType<typeof useProfilesQuery>);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProfileManageDialog
        isOpen
        onClose={vi.fn()}
        selectedApps={["claude", "codex", "kimi"]}
      />
    </QueryClientProvider>,
  );
  fireEvent.click(screen.getAllByTitle("重命名")[0]);
  fireEvent.change(screen.getByRole("textbox"), {
    target: { value: " Personal " },
  });
  fireEvent.click(screen.getByTitle("确定"));
  expect(screen.getByRole("alert")).toBeVisible();
  expect(screen.getByRole("textbox")).toHaveValue(" Personal ");
  expect(
    vi.mocked(useUpdateProfileMutation).mock.results[0].value.mutate,
  ).not.toHaveBeenCalled();
});

it("keeps a long profile name identifiable and allows cancelling deletion", async () => {
  const name = "LongProfileName".repeat(100);
  const data = vi.mocked(useProfilesQuery)().data!;
  vi.mocked(useProfilesQuery).mockReturnValue({
    data: { ...data, profiles: [{ ...data.profiles[0], name }] },
  } as ReturnType<typeof useProfilesQuery>);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProfileManageDialog
        isOpen
        onClose={vi.fn()}
        selectedApps={["claude", "codex", "kimi"]}
      />
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
      <ProfileManageDialog
        isOpen
        onClose={vi.fn()}
        selectedApps={["claude", "codex", "kimi"]}
      />
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
