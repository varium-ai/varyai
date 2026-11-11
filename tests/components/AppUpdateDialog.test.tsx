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
import { fetchReleaseNotes } from "@/lib/releaseNotes";

vi.mock("@/lib/releaseNotes", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/releaseNotes")>()),
  fetchReleaseNotes: vi.fn(),
}));

const state = vi.hoisted(() => ({
  hasUpdate: true,
  updateInfo: {
    currentVersion: "0.2.1",
    availableVersion: "0.3.0",
    notes:
      "## New features\n\n- Add **routing groups** and `model-a`.\n\n## Fixes\n\n- Keep saved accounts.",
  } as { currentVersion: string; availableVersion: string; notes?: string },
  isDismissed: false,
  isInstalling: false,
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
  state.hasUpdate = true;
  state.isDismissed = false;
  state.isInstalling = false;
  state.error = null;
  state.updateInfo.notes =
    "## New features\n\n- Add **routing groups** and `model-a`.\n\n## Fixes\n\n- Keep saved accounts.";
  vi.mocked(settingsApi.isPortable).mockResolvedValue(false);
  vi.mocked(settingsApi.openExternal).mockResolvedValue(undefined);
});

it("shows formatted release sections and the installed and available versions in the update dialog", async () => {
  mount();
  const dialog = await screen.findByRole("dialog");
  expect(
    within(dialog).getByRole("heading", { name: "New features" }),
  ).toBeVisible();
  expect(within(dialog).getByRole("heading", { name: "Fixes" })).toBeVisible();
  expect(within(dialog).getAllByRole("listitem")).toHaveLength(2);
  expect(within(dialog).getByText("routing groups").tagName).toBe("STRONG");
  expect(within(dialog).getByText("model-a").tagName).toBe("CODE");
  expect(dialog).toHaveTextContent("0.2.1");
  expect(dialog).toHaveTextContent("0.3.0");
  expect(
    within(dialog).getByRole("button", { name: "settings.restartToUpdate" }),
  ).toBeEnabled();
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
