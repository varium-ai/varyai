import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import i18n from "i18next";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import { APP_IDS, MANAGED_APP_IDS } from "@/config/appConfig";
import { ProfileSwitcher } from "@/components/profiles/ProfileSwitcher";
import { toast } from "sonner";
import { profilesApi } from "@/lib/api/profiles";
import type { AppId } from "@/lib/api/types";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

vi.mock("@/lib/api/profiles", () => ({
  profilesApi: {
    list: vi.fn(),
    captureBundle: vi.fn(),
    undoBundle: vi.fn(),
    apply: vi.fn(),
    create: vi.fn(),
    clearCurrent: vi.fn(),
  },
}));
vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getAll: vi
      .fn()
      .mockResolvedValue({ "saved-provider": { name: "Saved provider" } }),
    updateTrayMenu: vi.fn().mockResolvedValue(undefined),
  },
}));
const availableApps: AppId[] = APP_IDS.filter(
  (app) => app !== "doubaowork" && app !== "glm",
);

function mount(app: AppId, selectedApps: AppId[] = APP_IDS, compact = false) {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <ProfileSwitcher
        activeApp={app}
        selectedApps={selectedApps}
        compact={compact}
        side={compact ? "top" : "bottom"}
      />
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
      available: availableApps.includes(app),
      provider: availableApps.includes(app),
      mcp: false,
      skills: false,
      instructions: false,
      reason:
        app === "doubaowork"
          ? "profiles.bundle.nativeLoginOnly"
          : app === "glm"
            ? "profiles.bundle.nativeConfigUnsupported"
            : null,
    })),
  });
});

it("opens a compact empty profile menu without a redundant section separator", async () => {
  mount("claude", ["claude"], true);
  fireEvent.click(await screen.findByRole("combobox"));
  expect(await screen.findByText(zh.profiles.createFromCurrent)).toBeVisible();
  expect(document.querySelector("[cmdk-list] .bg-border")).toBeNull();
});

it("focuses the profile name on create and returns focus to the switcher on cancel", async () => {
  mount("claude", ["claude"], true);
  const switcher = await screen.findByRole("combobox");
  fireEvent.click(switcher);
  fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));

  const name = await screen.findByRole("textbox", { name: zh.profiles.name });
  await waitFor(() => expect(name).toHaveFocus());
  fireEvent.click(screen.getByRole("button", { name: zh.common.cancel }));
  await waitFor(() => expect(switcher).toHaveFocus());
});

it.each(["kimi", "claude", "opencode"] as const)(
  "shows the selected bundle from %s even when that agent has no snapshot",
  async (app) => {
    const data = await profilesApi.list();
    vi.mocked(profilesApi.list).mockResolvedValue({
      ...data,
      currentBundleId: "bundle-1",
      currentIds: {
        ...data.currentIds,
        byApp: { kimi: "bundle-2", claude: null, opencode: null },
      },
      profiles: [
        {
          id: "bundle-1",
          name: "Profile one",
          payload: { bundle: { selections: [] } },
        },
        {
          id: "bundle-2",
          name: "Profile two",
          payload: { bundle: { selections: [] } },
        },
      ],
    } as never);
    vi.mocked(profilesApi.clearCurrent).mockResolvedValue(undefined);
    mount(app);
    expect(
      await screen.findByRole("combobox", { name: /Profile one/ }),
    ).toHaveTextContent("Profile one");
    fireEvent.click(screen.getByRole("combobox"));
    fireEvent.click(await screen.findByText(zh.profiles.none));
    await waitFor(() =>
      expect(profilesApi.clearCurrent).toHaveBeenCalledWith("bundle"),
    );
  },
);

it("does not show an older per-agent bundle after clearing the shared selection", async () => {
  const data = await profilesApi.list();
  vi.mocked(profilesApi.list).mockResolvedValue({
    ...data,
    currentBundleId: null,
    currentIds: { ...data.currentIds, byApp: { kimi: "older-bundle" } },
    profiles: [
      {
        id: "older-bundle",
        name: "Older bundle",
        payload: { bundle: { selections: [] } },
      },
    ],
  } as never);
  mount("kimi");
  await waitFor(() => expect(profilesApi.list).toHaveBeenCalledTimes(2));
  expect(screen.getByRole("combobox")).toHaveTextContent(zh.profiles.none);
});

it("refreshes agent availability when reopening the profile menu", async () => {
  const data = await profilesApi.list();
  vi.mocked(profilesApi.list).mockResolvedValue({
    ...data,
    agents: data.agents!.map((agent) =>
      agent.app === "kimi"
        ? {
            ...agent,
            available: false,
            reason: "profiles.bundle.configureFirst",
          }
        : agent,
    ),
  });
  mount("kimi");
  await waitFor(() => expect(profilesApi.list).toHaveBeenCalledTimes(2));
  vi.mocked(profilesApi.list).mockResolvedValue(data);
  fireEvent.click(screen.getByRole("combobox"));
  fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
  fireEvent.click(
    screen.getByRole("checkbox", { name: zh.profiles.bundle.create }),
  );
  await waitFor(() =>
    expect(screen.getByRole("checkbox", { name: /Kimi Code/ })).toBeEnabled(),
  );
});

it.each([false, true])(
  "previews a legacy profile before applying, current=%s",
  async (current) => {
    const data = await profilesApi.list();
    vi.mocked(profilesApi.list).mockResolvedValue({
      ...data,
      currentIds: { ...data.currentIds, claude: current ? "legacy" : null },
      profiles: [
        {
          id: "legacy",
          name: "Legacy fixture",
          payload: {
            providers: {
              claude: "saved-provider",
              "claude-desktop": null,
              codex: null,
            },
            mcp: { claude: ["saved-mcp"], "claude-desktop": null, codex: null },
            skills: { claude: [], "claude-desktop": null, codex: null },
            prompts: {
              claude: "saved-prompt",
              "claude-desktop": null,
              codex: null,
            },
          },
        },
      ],
    });
    vi.mocked(profilesApi.apply).mockResolvedValue([]);
    mount("claude");
    fireEvent.click(await screen.findByRole("combobox"));
    fireEvent.click(
      await screen.findByRole("option", { name: "Legacy fixture" }),
    );
    expect(
      await screen.findByRole("dialog", { name: /Legacy fixture/ }),
    ).toBeVisible();
    expect(await screen.findByText("Saved provider")).toBeVisible();
    expect(screen.getByText("saved-mcp")).toBeVisible();
    expect(profilesApi.apply).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: zh.profiles.bundle.apply }),
    );
    await waitFor(() =>
      expect(profilesApi.apply).toHaveBeenCalledWith("legacy", "claude"),
    );
  },
);

it("rejects a duplicate trimmed profile name before capturing", async () => {
  const data = await profilesApi.list();
  vi.mocked(profilesApi.list).mockResolvedValue({
    ...data,
    profiles: [
      {
        id: "existing",
        name: "Work",
        payload: { bundle: { selections: [] } },
      } as never,
    ],
  });
  mount("kimi");
  fireEvent.click(await screen.findByRole("combobox"));
  fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
  fireEvent.change(screen.getByPlaceholderText(zh.profiles.namePlaceholder), {
    target: { value: " Work " },
  });
  fireEvent.click(screen.getByRole("button", { name: zh.common.confirm }));
  expect(screen.getByRole("alert")).toBeVisible();
  expect(profilesApi.captureBundle).not.toHaveBeenCalled();
  expect(profilesApi.create).not.toHaveBeenCalled();
});

it.each([false, true])(
  "clears a stale duplicate warning when the profile name changes (compact=%s)",
  async (compact) => {
    vi.mocked(profilesApi.captureBundle)
      .mockReset()
      .mockRejectedValueOnce(new Error("profiles.nameExists"))
      .mockResolvedValueOnce({ id: "created" } as Awaited<
        ReturnType<typeof profilesApi.captureBundle>
      >);
    mount("kimi", ["kimi"], compact);
    fireEvent.click(await screen.findByRole("combobox"));
    fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
    const name = screen.getByPlaceholderText(zh.profiles.namePlaceholder);
    const submit = screen.getByRole("button", { name: zh.common.confirm });
    fireEvent.change(name, { target: { value: "Newly taken name" } });
    fireEvent.click(submit);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      zh.profiles.nameExists,
    );
    expect(toast.error).not.toHaveBeenCalled();

    fireEvent.change(name, { target: { value: "Available name" } });
    expect(screen.queryByRole("alert")).toBeNull();
    fireEvent.click(submit);
    await waitFor(() =>
      expect(profilesApi.captureBundle).toHaveBeenLastCalledWith(
        "Available name",
        ["kimi"],
      ),
    );
    await waitFor(() => expect(name).not.toBeInTheDocument());
  },
);

it("opens an uncaptured client preview and closes without applying", async () => {
  const data = await profilesApi.list();
  const empty = { claude: null, "claude-desktop": null, codex: null };
  vi.mocked(profilesApi.list).mockResolvedValue({
    ...data,
    profiles: [
      {
        id: "legacy",
        name: "Uncaptured fixture",
        payload: {
          providers: { ...empty, claude: "saved-provider" },
          mcp: empty,
          skills: empty,
          prompts: empty,
        },
      },
    ],
  });
  mount("codex");
  fireEvent.click(await screen.findByRole("combobox"));
  fireEvent.click(
    await screen.findByRole("option", { name: /Uncaptured fixture/ }),
  );
  expect(
    await screen.findByRole("dialog", { name: /Uncaptured fixture/ }),
  ).toBeVisible();
  expect(screen.getByText(zh.profiles.noSnapshotForScope)).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: zh.common.close }));
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(profilesApi.apply).not.toHaveBeenCalled();
});
describe("Profile entry for added agents", () => {
  it.each(["gemini", "kimi", "qoder", "trae", "zcode"] as const)(
    "offers cross-agent profiles from %s",
    async (app) => {
      mount(app);
      fireEvent.click(await screen.findByRole("combobox"));
      fireEvent.click(await screen.findByText("新建配置方案"));
      fireEvent.click(
        screen.getByRole("checkbox", { name: zh.profiles.bundle.create }),
      );
      expect(screen.getByRole("checkbox", { name: /Gemini/ })).toBeEnabled();
      expect(screen.getByLabelText("Qoder CN", { exact: false })).toBeEnabled();
      expect(screen.getByLabelText("TRAE CN", { exact: false })).toBeEnabled();
      expect(
        screen.getByText(zh.profiles.bundle.nativeLoginOnly),
      ).toBeInTheDocument();
    },
  );
});

it.each(availableApps)(
  "captures only the current agent by default from %s",
  async (app) => {
    mount(app, [app, "codex", "kimi"]);
    fireEvent.click(await screen.findByRole("combobox"));
    fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
    const multiAgent = screen.getByRole("checkbox", {
      name: zh.profiles.bundle.create,
    });
    expect(multiAgent).toBeEnabled();
    expect(multiAgent).not.toBeChecked();
    fireEvent.change(screen.getByPlaceholderText(zh.profiles.namePlaceholder), {
      target: { value: "Current agent fixture" },
    });
    fireEvent.click(screen.getByRole("button", { name: zh.common.confirm }));
    await waitFor(() =>
      expect(profilesApi.captureBundle).toHaveBeenCalledWith(
        "Current agent fixture",
        [app],
      ),
    );
    expect(profilesApi.create).not.toHaveBeenCalled();
  },
);

it("includes only selected, available agents when multi-agent capture is enabled", async () => {
  const data = await profilesApi.list();
  vi.mocked(profilesApi.list).mockResolvedValue({
    ...data,
    agents: data.agents!.map((agent) =>
      agent.app === "qoder" ? { ...agent, available: false } : agent,
    ),
  });
  mount("kimi", ["kimi", "codex", "qoder"]);
  fireEvent.click(await screen.findByRole("combobox"));
  fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
  fireEvent.click(
    screen.getByRole("checkbox", { name: zh.profiles.bundle.create }),
  );
  expect(screen.getByRole("checkbox", { name: /Kimi Code/ })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: /Codex/ })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: /Gemini/ })).not.toBeChecked();
  expect(screen.getByRole("checkbox", { name: /Qoder CN/ })).not.toBeChecked();
  fireEvent.change(screen.getByPlaceholderText(zh.profiles.namePlaceholder), {
    target: { value: "Selected agents fixture" },
  });
  fireEvent.click(screen.getByRole("button", { name: zh.common.confirm }));
  await waitFor(() =>
    expect(profilesApi.captureBundle).toHaveBeenCalledWith(
      "Selected agents fixture",
      ["codex", "kimi"],
    ),
  );
});

it("returns to current-agent capture after disabling multi-agent mode", async () => {
  mount("kimi", ["kimi", "codex"]);
  fireEvent.click(await screen.findByRole("combobox"));
  fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
  const multiAgent = screen.getByRole("checkbox", {
    name: zh.profiles.bundle.create,
  });
  fireEvent.click(multiAgent);
  fireEvent.click(multiAgent);
  fireEvent.change(screen.getByPlaceholderText(zh.profiles.namePlaceholder), {
    target: { value: "Current only fixture" },
  });
  fireEvent.click(screen.getByRole("button", { name: zh.common.confirm }));
  await waitFor(() =>
    expect(profilesApi.captureBundle).toHaveBeenCalledWith(
      "Current only fixture",
      ["kimi"],
    ),
  );
});

it("does not capture other agents when the current agent is unavailable", async () => {
  const data = await profilesApi.list();
  vi.mocked(profilesApi.list).mockResolvedValue({
    ...data,
    agents: data.agents!.map((agent) =>
      agent.app === "kimi" ? { ...agent, available: false } : agent,
    ),
  });
  mount("kimi", ["kimi", "codex"]);
  fireEvent.click(await screen.findByRole("combobox"));
  fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
  fireEvent.change(screen.getByPlaceholderText(zh.profiles.namePlaceholder), {
    target: { value: "Unavailable fixture" },
  });
  expect(
    screen.getByRole("button", { name: zh.common.confirm }),
  ).toBeDisabled();
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

it.each([
  { multi: false, composing: false },
  { multi: false, composing: true },
  { multi: true, composing: false },
  { multi: true, composing: true },
])(
  "requires clicking confirm to save a profile (multi=$multi, composing=$composing)",
  async ({ multi, composing }) => {
    mount("claude", ["claude", "pi"]);
    fireEvent.click(await screen.findByRole("combobox"));
    fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
    if (multi) {
      fireEvent.click(
        screen.getByRole("checkbox", { name: zh.profiles.bundle.create }),
      );
    }
    const input = screen.getByPlaceholderText(zh.profiles.namePlaceholder);
    if (composing) fireEvent.compositionStart(input);
    fireEvent.change(input, { target: { value: "测试方案" } });
    await act(async () => {
      fireEvent.keyDown(input, {
        key: "Enter",
        code: "Enter",
        keyCode: composing ? 229 : 13,
        isComposing: composing,
      });
      if (composing) {
        fireEvent.compositionEnd(input, { data: "测试方案" });
      }
    });
    expect(profilesApi.captureBundle).not.toHaveBeenCalled();
    expect(profilesApi.create).not.toHaveBeenCalled();
    expect(input).toBeVisible();
    expect(input).toHaveValue("测试方案");
    fireEvent.click(screen.getByRole("button", { name: zh.common.confirm }));
    await waitFor(() =>
      expect(profilesApi.captureBundle).toHaveBeenCalledWith(
        "测试方案",
        multi ? ["claude", "pi"] : ["claude"],
      ),
    );
    expect(profilesApi.captureBundle).toHaveBeenCalledTimes(1);
  },
);

it.each([
  ["en", en],
  ["ja", ja],
  ["zh", zh],
  ["zh-TW", zhTW],
] as const)(
  "provides labels for the complete Agent inventory in %s",
  (_language, locale) => {
    for (const app of APP_IDS) {
      expect(locale.apps, app).toHaveProperty(app, expect.stringMatching(/\S/));
    }
  },
);

it.each(availableApps)(
  "captures %s directly through the application single-Agent picker",
  async (app) => {
    mount("claude");
    fireEvent.click(await screen.findByRole("combobox"));
    fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
    const picker = screen.getByRole("combobox", {
      name: zh.profiles.bundle.singleAgent,
    });
    fireEvent.keyDown(picker, { key: "ArrowDown" });
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual(
      MANAGED_APP_IDS.map((app) => zh.apps[app]),
    );
    expect(
      screen.getByRole("option", { name: zh.apps.doubaowork }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.queryByRole("option", { name: zh.apps.glm }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: zh.apps[app] }));
    expect(
      screen.getByRole("checkbox", { name: zh.profiles.bundle.create }),
    ).not.toBeChecked();
    fireEvent.change(screen.getByPlaceholderText(zh.profiles.namePlaceholder), {
      target: { value: "Single Pi fixture" },
    });
    fireEvent.click(screen.getByRole("button", { name: zh.common.confirm }));
    await waitFor(() =>
      expect(profilesApi.captureBundle).toHaveBeenCalledWith(
        "Single Pi fixture",
        [app],
      ),
    );
  },
);

it("keeps the compact profile picker native and unavailable Agents disabled", async () => {
  mount("claude", APP_IDS, true);
  fireEvent.click(await screen.findByRole("combobox"));
  fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
  const select = screen.getByLabelText(zh.profiles.bundle.singleAgent);
  expect(select).toBeInstanceOf(HTMLSelectElement);
  expect(
    screen.getByRole("option", { name: zh.apps.doubaowork }),
  ).toBeDisabled();
  expect(
    screen.queryByRole("option", { name: zh.apps.glm }),
  ).not.toBeInTheDocument();
});

it("does not flash duplicate-name error after saving a new profile while post-create refresh is in flight", async () => {
  const { providersApi } = await import("@/lib/api/providers");
  const initial = await profilesApi.list();
  let finishTrayUpdate!: () => void;
  vi.mocked(providersApi.updateTrayMenu).mockImplementationOnce(
    () =>
      new Promise<boolean>((resolve) => {
        finishTrayUpdate = () => resolve(true);
      }),
  );
  vi.mocked(profilesApi.captureBundle).mockImplementationOnce(async () => {
    vi.mocked(profilesApi.list).mockResolvedValueOnce({
      ...initial,
      profiles: [
        {
          id: "bundle-new",
          name: "测试3",
          payload: { bundle: { selections: [] } },
        },
      ],
    } as never);
    return {
      id: "bundle-new",
      name: "测试3",
      payload: { bundle: { selections: [] } },
    } as never;
  });

  mount("claude");
  fireEvent.click(await screen.findByRole("combobox"));
  fireEvent.click(await screen.findByText(zh.profiles.createFromCurrent));
  fireEvent.change(screen.getByPlaceholderText(zh.profiles.namePlaceholder), {
    target: { value: "测试3" },
  });
  fireEvent.click(screen.getByRole("button", { name: zh.common.confirm }));

  await waitFor(() =>
    expect(providersApi.updateTrayMenu).toHaveBeenCalledTimes(1),
  );
  expect(screen.queryByText(zh.profiles.nameExists)).toBeNull();

  await act(async () => {
    finishTrayUpdate();
  });
  await waitFor(() =>
    expect(
      screen.queryByPlaceholderText(zh.profiles.namePlaceholder),
    ).toBeNull(),
  );
});
