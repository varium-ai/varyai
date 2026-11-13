import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { AppUpdateDialog } from "@/components/AppUpdateDialog";
import { settingsApi } from "@/lib/api";
import { toast } from "sonner";
import { fetchReleaseNotes, fetchReleaseHistory } from "@/lib/releaseNotes";

vi.mock("@/lib/releaseNotes", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/releaseNotes")>()),
  fetchReleaseNotes: vi.fn(),
  fetchReleaseHistory: vi.fn(),
}));

const state = vi.hoisted(() => ({
  language: "en",
  hasUpdate: true,
  updateInfo: {
    currentVersion: "0.2.1",
    availableVersion: "0.3.0",
    notes:
      "## New features\n\n- Add **routing groups** and `model-a`.\n\n## Fixes\n\n- Keep saved accounts.",
  } as { currentVersion: string; availableVersion: string; notes?: string },
  isDismissed: false,
  isInstalling: false,
  installProgress: null as import("@/lib/updater").UpdateInstallProgress | null,
  isDownloadStalled: false,
  isCancelling: false,
  cancelUpdate: vi.fn(),
  error: null as string | null,
  dismissUpdate: vi.fn(),
  installUpdate: vi.fn(),
}));
vi.mock("@/contexts/UpdateContext", () => ({ useUpdate: () => state }));
vi.mock("@/lib/api", () => ({
  settingsApi: { isPortable: vi.fn(), openExternal: vi.fn() },
}));
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    i18n: { language: state.language },
    t: (key: string, values?: { version?: string }) =>
      values?.version ? `${key} ${values.version}` : key,
  }),
}));

function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <AppUpdateDialog />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(fetchReleaseNotes).mockResolvedValue("");
  vi.mocked(fetchReleaseHistory).mockResolvedValue({
    releases: [],
    complete: true,
  });
  state.updateInfo.currentVersion = "0.2.1";
  state.updateInfo.availableVersion = "0.3.0";
  state.hasUpdate = true;
  state.isDismissed = false;
  state.isInstalling = false;
  state.installProgress = null;
  state.isDownloadStalled = false;
  state.isCancelling = false;
  state.error = null;
  state.language = "en";
  state.updateInfo.notes =
    "## New features\n\n- Add **routing groups** and `model-a`.\n\n## Fixes\n\n- Keep saved accounts.";
  vi.mocked(settingsApi.isPortable).mockResolvedValue(false);
  vi.mocked(settingsApi.openExternal).mockResolvedValue(undefined);
});

it("includes skipped releases with their own versions when updating from 0.3.0 to 0.3.4", async () => {
  state.language = "zh";
  state.updateInfo.currentVersion = "0.3.0";
  state.updateInfo.availableVersion = "0.3.4";
  state.updateInfo.notes =
    "## Fixes\n- Target English.\n<!-- lang:zh -->\n## 问题修复\n- 最新修复。";
  vi.mocked(fetchReleaseHistory).mockResolvedValue({
    releases: [
      { version: "0.3.4", notes: "## Fixes\n- Stale target description." },
      {
        version: "0.3.3",
        notes:
          "## Features\n- Old English.\n<!-- lang:zh -->\n## 新增功能\n- 独立更新。",
      },
      {
        version: "0.3.2",
        notes:
          "## Improvements\n- Earlier English.\n<!-- lang:zh -->\n## 改进\n- 更新按钮位置。",
      },
    ],
    complete: true,
  });
  mount();
  await screen.findByText("独立更新。");
  for (const [text, version] of [
    ["最新修复。", "v0.3.4"],
    ["独立更新。", "v0.3.3"],
    ["更新按钮位置。", "v0.3.2"],
  ]) {
    const item = screen.getByText(text).closest("li");
    expect(item).toHaveTextContent(version);
  }
  expect(screen.queryByText("Old English.")).toBeNull();
  expect(screen.queryByText("Stale target description.")).toBeNull();
  expect(fetchReleaseHistory).toHaveBeenCalledWith(
    "0.3.0",
    "0.3.4",
    expect.any(AbortSignal),
  );
});

it("keeps target notes and installation available while history is unavailable", async () => {
  vi.mocked(fetchReleaseHistory).mockResolvedValue({
    releases: [],
    complete: false,
  });
  mount();
  expect(
    await screen.findByText("settings.releaseHistoryIncomplete"),
  ).toBeVisible();
  expect(screen.getByRole("heading", { name: "New features" })).toBeVisible();
  expect(
    screen.getByRole("button", { name: "settings.restartToUpdate" }),
  ).toBeEnabled();
});

it("resolves links in older notes against that release rather than the target", async () => {
  vi.mocked(fetchReleaseHistory).mockResolvedValue({
    releases: [
      { version: "0.2.2", notes: "## Fixes\n- See [older details](#details)." },
    ],
    complete: true,
  });
  mount();
  fireEvent.click(await screen.findByRole("link", { name: "older details" }));
  expect(settingsApi.openExternal).toHaveBeenCalledWith(
    "https://github.com/varium-ai/varyai/releases/tag/v0.2.2#details",
  );
});

it("shows formatted release sections and the installed and available versions in the update dialog", async () => {
  mount();
  const dialog = await screen.findByRole("dialog");
  expect(
    within(dialog).getByRole("heading", { name: "New features" }),
  ).toBeVisible();
  expect(within(dialog).getByRole("heading", { name: "Fixes" })).toBeVisible();
  expect(within(dialog).getAllByRole("listitem")).toHaveLength(2);
  for (const item of within(dialog).getAllByRole("listitem")) {
    expect(within(item).getByText("v0.3.0")).toBeVisible();
  }
  expect(within(dialog).getByText("routing groups").tagName).toBe("STRONG");
  expect(within(dialog).getByText("model-a").tagName).toBe("CODE");
  expect(dialog).toHaveTextContent("0.2.1");
  expect(dialog).toHaveTextContent("0.3.0");
  expect(
    within(dialog).getByRole("button", { name: "settings.restartToUpdate" }),
  ).toBeEnabled();
});

it("updates manifest notes when the interface language changes", async () => {
  state.updateInfo.notes =
    "## Features\n- English change.\n<!-- lang:zh -->\n## 新增功能\n- 中文变更。";
  state.language = "zh";
  const view = mount();
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("中文变更。");
  expect(dialog).not.toHaveTextContent("English change.");
  expect(
    within(within(dialog).getByRole("listitem")).getByText("v0.3.0"),
  ).toBeVisible();
  state.language = "en";
  view.rerender(
    <QueryClientProvider client={new QueryClient()}>
      <AppUpdateDialog />
    </QueryClientProvider>,
  );
  expect(dialog).toHaveTextContent("English change.");
  expect(dialog).not.toHaveTextContent("中文变更。");
  expect(fetchReleaseNotes).not.toHaveBeenCalled();
});

it("localizes fetched notes and labels them with the fetched release version", async () => {
  state.updateInfo.notes = undefined;
  state.language = "ja";
  vi.mocked(fetchReleaseNotes).mockResolvedValue(
    "## Features\n- English change.\n<!-- lang:ja -->\n## 新機能\n- 日本語の変更。",
  );
  mount();
  expect(await screen.findByRole("heading", { name: "新機能" })).toBeVisible();
  const item = screen.getByRole("listitem");
  expect(item).toHaveTextContent("日本語の変更。");
  expect(within(item).getByText("v0.3.0")).toBeVisible();
  expect(screen.queryByText("English change.")).toBeNull();
});

it("opens release-note links externally without navigating the app or rendering remote images or HTML", async () => {
  state.updateInfo.notes =
    "## Fixes\n\n- Read [details](https://example.test/notes).\n\n![tracking](https://example.test/pixel.png)\n\n<script>bad()</script>\n\n[unsafe](javascript:alert(1))";
  mount();
  const dialog = await screen.findByRole("dialog");
  const link = within(dialog).getByRole("link", { name: "details" });
  fireEvent.click(link);
  await waitFor(() =>
    expect(settingsApi.openExternal).toHaveBeenCalledWith(
      "https://example.test/notes",
    ),
  );
  expect(dialog.querySelector("img, script, iframe")).toBeNull();
  expect(within(dialog).queryByRole("link", { name: "unsafe" })).toBeNull();
});

it("keeps update and dismissal actions available without release notes", async () => {
  state.updateInfo.notes = undefined;
  mount();
  const dialog = await screen.findByRole("dialog");
  expect(
    await within(dialog).findByText("settings.releaseNotesUnavailable"),
  ).toBeVisible();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "settings.restartToUpdate" }),
  );
  expect(state.installUpdate).toHaveBeenCalledOnce();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "settings.updateLater" }),
  );
  expect(state.dismissUpdate).toHaveBeenCalledOnce();
});

it("uses manual download for portable builds and does not run the installer", async () => {
  vi.mocked(settingsApi.isPortable).mockResolvedValue(true);
  mount();
  const button = await screen.findByRole("button", {
    name: "settings.downloadUpdate",
  });
  fireEvent.click(button);
  await waitFor(() =>
    expect(settingsApi.openExternal).toHaveBeenCalledWith(
      "https://github.com/varium-ai/varyai/releases/latest",
    ),
  );
  expect(state.installUpdate).not.toHaveBeenCalled();
});

it("locks dismissal during installation and reports a failed install in a toast", async () => {
  state.installUpdate.mockRejectedValueOnce(
    new Error("synthetic-private-detail"),
  );
  const view = mount();
  fireEvent.click(
    await screen.findByRole("button", { name: "settings.restartToUpdate" }),
  );
  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("settings.updateInstallFailed"),
  );
  expect(screen.queryByText("synthetic-private-detail")).toBeNull();
  state.isInstalling = true;
  view.rerender(
    <QueryClientProvider client={new QueryClient()}>
      <AppUpdateDialog />
    </QueryClientProvider>,
  );
  expect(
    screen.getByRole("button", { name: "settings.updateLater" }),
  ).toBeDisabled();
  expect(
    screen.getByRole("button", { name: "settings.updating" }),
  ).toBeDisabled();
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(state.dismissUpdate).not.toHaveBeenCalled();
});

it("retrieves the matching release when an older manifest only supplies placeholder text", async () => {
  state.updateInfo.notes = "See the GitHub release page for release notes.";
  vi.mocked(fetchReleaseNotes).mockResolvedValue(
    "## Improvements\n\n- Make updates readable.",
  );
  mount();
  expect(
    await screen.findByRole("heading", { name: "Improvements" }),
  ).toBeVisible();
  expect(fetchReleaseNotes).toHaveBeenCalledWith(
    "0.3.0",
    expect.any(AbortSignal),
  );
  expect(screen.queryByText(state.updateInfo.notes)).toBeNull();
});
it("uses manifest notes immediately without another network request", async () => {
  mount();
  await screen.findByRole("heading", { name: "New features" });
  expect(fetchReleaseNotes).not.toHaveBeenCalled();
});

it("keeps installation available when the optional release-notes request fails", async () => {
  state.updateInfo.notes = undefined;
  vi.mocked(fetchReleaseNotes).mockRejectedValue(
    new Error("synthetic request failed"),
  );
  mount();
  expect(
    await screen.findByText("settings.releaseNotesUnavailable"),
  ).toBeVisible();
  expect(
    screen.getByRole("button", { name: "settings.restartToUpdate" }),
  ).toBeEnabled();
  expect(screen.queryByRole("alert")).toBeNull();
});

it("shows byte progress and a cancellation action while downloading", async () => {
  state.isInstalling = true;
  state.installProgress = {
    phase: "downloading",
    downloaded: 1048576,
    total: 4194304,
  };
  mount();
  const bar = await screen.findByRole("progressbar");
  expect(bar).toHaveAttribute("value", "25");
  expect(screen.getByText(/1 MB \/ 4 MB/)).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", { name: "settings.updateCancel" }),
  );
  expect(state.cancelUpdate).toHaveBeenCalledOnce();
  expect(state.dismissUpdate).not.toHaveBeenCalled();
});

it("uses indeterminate progress for unknown sizes and explains stalled downloads", async () => {
  state.isInstalling = true;
  state.installProgress = {
    phase: "downloading",
    downloaded: 2048,
    total: null,
  };
  state.isDownloadStalled = true;
  mount();
  expect(await screen.findByRole("progressbar")).not.toHaveAttribute("value");
  expect(screen.getByText(/2 KB/)).toBeVisible();
  expect(screen.getByText("settings.updateDownloadStalled")).toBeVisible();
  expect(
    screen.getByRole("button", { name: "settings.updateCancel" }),
  ).toBeEnabled();
});

it("locks cancellation once installation starts and keeps status visible in the footer", async () => {
  state.isInstalling = true;
  state.installProgress = { phase: "installing", downloaded: 100, total: 100 };
  mount();
  const status = await screen.findByText("settings.updateInstalling");
  expect(status.closest(".vary-dialog-footer")).not.toBeNull();
  expect(
    screen.queryByRole("button", { name: "settings.updateCancel" }),
  ).toBeNull();
  expect(
    screen.getByRole("button", { name: "settings.updateLater" }),
  ).toBeDisabled();
});

it("reports cancellation failure without exposing backend details", async () => {
  state.isInstalling = true;
  state.installProgress = { phase: "checking", downloaded: 0, total: null };
  state.cancelUpdate.mockRejectedValueOnce(
    new Error("synthetic-private-detail"),
  );
  mount();
  fireEvent.click(
    await screen.findByRole("button", { name: "settings.updateCancel" }),
  );
  await waitFor(() =>
    expect(toast.error).toHaveBeenCalledWith("settings.updateCancelFailed"),
  );
  expect(screen.queryByText("synthetic-private-detail")).toBeNull();
});
