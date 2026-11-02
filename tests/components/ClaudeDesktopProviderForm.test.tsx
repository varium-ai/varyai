import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ClaudeDesktopProviderForm } from "@/components/providers/forms/ClaudeDesktopProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
import { detectProviderProtocols } from "@/lib/api/provider-protocol-detection";
import { providersApi } from "@/lib/api/providers";

vi.mock("@/lib/api/provider-protocol-detection", () => ({
  detectProviderProtocols: vi.fn().mockResolvedValue([]),
}));

vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/model-fetch")>()),
  fetchModelsForConfig: vi.fn().mockResolvedValue([]),
}));

const authState = vi.hoisted(() => ({
  codexReauthRequired: false,
}));
const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: toastMocks.error,
    success: vi.fn(),
  },
}));

vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  return {
    ...actual,
    useCopilotAuth: () => ({
      isAuthenticated: false,
      accounts: [],
    }),
    useCodexOauth: () => ({
      isAuthenticated: true,
      defaultAccountId: "acct-managed",
      accounts: [
        {
          id: "acct-managed",
          is_default: true,
          reauth_required: authState.codexReauthRequired,
          requires_reauth: false,
        },
      ],
    }),
    useXaiOauth: () => ({
      isAuthenticated: false,
      accounts: [],
    }),
  };
});

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex-oauth-section" />,
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div data-testid="copilot-auth-section" />,
}));

vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => <div data-testid="xai-oauth-section" />,
}));

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getClaudeDesktopDefaultRoutes: vi.fn().mockResolvedValue([]),
  },
}));

function renderForm(
  initialData: ComponentProps<typeof ClaudeDesktopProviderForm>["initialData"],
  onSubmit = vi.fn(),
) {
  const queryClient = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <ClaudeDesktopProviderForm
        submitLabel="保存"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={initialData}
      />
    </QueryClientProvider>,
  );
  return { ...view, onSubmit };
}

describe("ClaudeDesktopProviderForm", () => {
  it("fetches mapped Gemini models using the native protocol", async () => {
    renderForm({
      name: "Gemini fixture",
      category: "third_party",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://fixture.example",
          ANTHROPIC_API_KEY: "synthetic-key",
        },
      },
      meta: { claudeDesktopMode: "proxy", apiFormat: "gemini_native" },
    });
    fireEvent.click(screen.getByRole("button", { name: "获取模型" }));
    await waitFor(() =>
      expect(fetchModelsForConfig).toHaveBeenCalledWith(
        "https://fixture.example",
        "synthetic-key",
        undefined,
        undefined,
        undefined,
        { apiFormat: "google-generative-ai" },
      ),
    );
  });

  beforeEach(() => {
    authState.codexReauthRequired = false;
    vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
    vi.mocked(detectProviderProtocols).mockReset().mockResolvedValue([]);
    vi.mocked(providersApi.getClaudeDesktopDefaultRoutes).mockResolvedValue([]);
  });

  it("keeps automatic protocol selection enabled after switching model mapping", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "synthetic-glm", ownedBy: null },
    ]);
    vi.mocked(detectProviderProtocols).mockResolvedValue([
      {
        format: "openai_chat",
        text: { status: "verified" },
        stream: { status: "verified" },
      },
      {
        format: "anthropic",
        text: { status: "verified" },
        stream: { status: "verified" },
      },
    ]);
    const view = renderForm(undefined);
    fireEvent.change(view.container.querySelector("#baseUrl")!, {
      target: { value: "https://fixture.example/v1" },
    });
    fireEvent.change(view.container.querySelector("#apiKey")!, {
      target: { value: "synthetic-key" },
    });
    await waitFor(
      () => {
        expect(detectProviderProtocols).toHaveBeenCalledWith(
          expect.objectContaining({ model: "synthetic-glm" }),
        );
        expect(
          screen
            .getAllByRole("combobox")
            .some((element) =>
              element.textContent?.includes("OpenAI Chat Completions"),
            ),
        ).toBe(true);
      },
      { timeout: 3500 },
    );
  });

  it("keeps the chosen detection model without filling the saved model mappings", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    const user = userEvent.setup();
    vi.mocked(providersApi.getClaudeDesktopDefaultRoutes).mockResolvedValue([
      {
        routeId: "claude-sonnet-5",
        envKey: "ANTHROPIC_DEFAULT_SONNET_MODEL",
        supports1m: false,
      },
      {
        routeId: "claude-opus-5",
        envKey: "ANTHROPIC_DEFAULT_OPUS_MODEL",
        supports1m: false,
      },
    ]);
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "qwen-unavailable", ownedBy: null },
      { id: "glm-working", ownedBy: null },
    ]);
    vi.mocked(detectProviderProtocols).mockImplementation(async ({ model }) =>
      model === "glm-working"
        ? [
            {
              format: "openai_chat",
              text: { status: "verified" },
              stream: { status: "verified" },
            },
          ]
        : [],
    );
    const view = renderForm(undefined);
    fireEvent.change(view.container.querySelector("#baseUrl")!, {
      target: { value: "https://fixture.example/v1" },
    });
    fireEvent.change(view.container.querySelector("#apiKey")!, {
      target: { value: "synthetic-key" },
    });
    await waitFor(
      () =>
        expect(detectProviderProtocols).toHaveBeenCalledWith(
          expect.objectContaining({ model: "qwen-unavailable" }),
        ),
      { timeout: 3500 },
    );
    const detection = view.container.querySelector("section")!;
    await user.click(
      within(detection).getByRole("button", { name: "Select detection model" }),
    );
    await user.click(
      await screen.findByRole("option", { name: "glm-working" }),
    );
    await waitFor(
      () => {
        expect(detectProviderProtocols).toHaveBeenCalledWith(
          expect.objectContaining({ model: "glm-working" }),
        );
        expect(
          screen.queryByRole("combobox", { name: "接入方式" }),
        ).not.toBeInTheDocument();
      },
      { timeout: 3500 },
    );
    expect(
      screen.getAllByRole("textbox", { name: "实际请求模型" })[0],
    ).toHaveValue("");
    expect(
      screen.getAllByRole("textbox", { name: "菜单显示名" })[0],
    ).toHaveValue("");
    expect(
      screen.getAllByRole("textbox", { name: "菜单显示名" })[1],
    ).toHaveValue("");
    fireEvent.click(
      within(detection).getByRole("button", {
        name: "providerDetection.check",
      }),
    );
    await waitFor(() =>
      expect(detectProviderProtocols).toHaveBeenLastCalledWith(
        expect.objectContaining({ model: "glm-working" }),
      ),
    );
  });

  it.each(["github_copilot", "codex_oauth", "xai_oauth"])(
    "托管 OAuth %s 即使旧数据是 direct 也强制开启模型映射",
    (providerType) => {
      renderForm({
        name: "Managed OAuth Provider",
        category: "third_party",
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://api.example.com",
          },
        },
        meta: {
          providerType,
          claudeDesktopMode: "direct",
          apiFormat: "anthropic",
          claudeDesktopModelRoutes: {
            "claude-sonnet-5": { model: "upstream-model" },
          },
        },
      });

      expect(
        screen.queryByRole("combobox", { name: "接入方式" }),
      ).not.toBeInTheDocument();
    },
  );

  it("uses one model editor and leaves routing to the external controls", () => {
    renderForm(undefined);
    expect(
      screen.queryByRole("combobox", { name: "接入方式" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("模型角色")).toBeInTheDocument();
  });

  it("已添加的第三方直连供应商仍可编辑模型列表", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "PackyCode",
        category: "third_party",
        settingsConfig: {
          env: { ANTHROPIC_BASE_URL: "https://www.packyapi.ai" },
        },
        meta: {
          claudeDesktopMode: "direct",
          apiFormat: "anthropic",
          claudeDesktopModelRoutes: {
            "claude-sonnet-5": { model: "claude-sonnet-5" },
            "claude-opus-5": { model: "claude-opus-5" },
            "claude-haiku-4-5": { model: "claude-haiku-4-5" },
          },
        },
      },
      onSubmit,
    );

    expect(screen.getByDisplayValue("claude-sonnet-5")).toBeInTheDocument();
    expect(screen.getByDisplayValue("claude-opus-5")).toBeInTheDocument();
    expect(screen.getByDisplayValue("claude-haiku-4-5")).toBeInTheDocument();

    const sonnetInput = screen.getByDisplayValue("claude-sonnet-5");
    await user.clear(sonnetInput);
    await user.type(sonnetInput, "claude-sonnet-5-custom");
    await user.type(screen.getByLabelText("API Key"), "sk-test");
    await user.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    expect(
      onSubmit.mock.calls[0][0].meta.claudeDesktopModelRoutes,
    ).toMatchObject({
      "claude-opus-5": { model: "claude-opus-5" },
      "claude-haiku-4-5": { model: "claude-haiku-4-5" },
      "claude-sonnet-5": { model: "claude-sonnet-5-custom" },
    });
    expect(
      onSubmit.mock.calls[0][0].meta.claudeDesktopModelRoutes,
    ).not.toHaveProperty("claude-sonnet-5-custom");
  });

  it("preserves saved model mappings in the unified editor", async () => {
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    renderForm({
      name: "Proxy Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      },
      meta: {
        claudeDesktopMode: "proxy",
        claudeDesktopModelRoutes: {
          "claude-sonnet-5": {
            model: "upstream-sonnet",
          },
        },
      },
    });

    expect(screen.getByDisplayValue("upstream-sonnet")).toBeInTheDocument();
    expect(
      screen.queryByRole("combobox", { name: "接入方式" }),
    ).not.toBeInTheDocument();
  });

  it("编辑模型映射的菜单显示名时保持输入框焦点", () => {
    renderForm({
      name: "Proxy Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      },
      meta: {
        claudeDesktopMode: "proxy",
        claudeDesktopModelRoutes: {
          "claude-old": {
            model: "upstream-old",
          },
        },
      },
    });

    const input = screen.getAllByPlaceholderText(
      "DeepSeek V4 Pro",
    )[0] as HTMLInputElement;
    input.focus();

    fireEvent.change(input, { target: { value: "DeepSeek V4 Pro" } });

    const currentInput = screen.getAllByPlaceholderText(
      "DeepSeek V4 Pro",
    )[0] as HTMLInputElement;
    expect(currentInput).toHaveValue("DeepSeek V4 Pro");
    expect(document.activeElement).toBe(currentInput);
  });

  it("编辑直连模型列表的模型 ID 时保持输入框焦点", () => {
    renderForm({
      name: "Direct Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      },
      meta: {
        claudeDesktopMode: "direct",
        claudeDesktopModelRoutes: {
          "claude-old": {
            model: "claude-old",
          },
        },
      },
    });

    const input = screen.getByRole("textbox", {
      name: "实际请求模型",
    }) as HTMLInputElement;
    input.focus();

    fireEvent.change(input, { target: { value: "claude-12345" } });

    const currentInput = screen.getByRole("textbox", {
      name: "实际请求模型",
    }) as HTMLInputElement;
    expect(currentInput).toHaveValue("claude-12345");
    expect(document.activeElement).toBe(currentInput);
  });

  it("only renders the saved proxy mappings without adding empty tiers", () => {
    renderForm({
      name: "Proxy Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      },
      meta: {
        claudeDesktopMode: "proxy",
        claudeDesktopModelRoutes: {
          "claude-sonnet-4-6": { model: "upstream-sonnet" },
        },
      },
    });

    expect(
      screen.getAllByPlaceholderText(/DeepSeek V4 (Pro|Flash)/),
    ).toHaveLength(1);
  });

  it("preserves seven mappings and repeated roles through mode changes, saving and reopening", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    const mappings = Object.fromEntries(
      [
        "claude-sonnet-5",
        "claude-opus-5",
        "claude-fable-5",
        "claude-haiku-4-5",
        "claude-sonnet-5-r2",
        "claude-sonnet-4-6",
        "claude-opus-5-r2",
      ].map((route, index) => [
        route,
        {
          model: `model-${index}`,
          labelOverride: `GLM ${index}`,
          supports1m: true,
        },
      ]),
    );
    const initialData = {
      name: "Seven models",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://fixture.example/v1",
          ANTHROPIC_AUTH_TOKEN: "synthetic-key",
        },
      },
      meta: {
        claudeDesktopMode: "proxy" as const,
        claudeDesktopModelRoutes: mappings,
      },
    };
    const view = renderForm(initialData);
    expect(
      screen.getAllByRole("textbox", { name: "实际请求模型" }),
    ).toHaveLength(7);
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(view.onSubmit).toHaveBeenCalledOnce());
    const saved = view.onSubmit.mock.calls[0][0];
    expect(saved.meta.claudeDesktopModelRoutes).toEqual(mappings);
    view.unmount();
    renderForm({ ...initialData, meta: saved.meta });
    expect(
      screen.getAllByRole("textbox", { name: "实际请求模型" }),
    ).toHaveLength(7);
    expect(screen.getByDisplayValue("GLM 6")).toBeInTheDocument();
  });

  it("adds more than four mappings, changes roles without collisions and deletes one mapping", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    const user = userEvent.setup();
    const view = renderForm({
      name: "Multiple models",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://fixture.example/v1",
          ANTHROPIC_AUTH_TOKEN: "synthetic-key",
        },
      },
      meta: { claudeDesktopMode: "proxy", claudeDesktopModelRoutes: {} },
    });
    for (let index = 0; index < 6; index += 1) {
      fireEvent.click(screen.getByRole("button", { name: "添加模型" }));
      fireEvent.change(
        screen.getAllByRole("textbox", { name: "实际请求模型" })[index],
        {
          target: { value: `upstream-${index}` },
        },
      );
    }
    await user.click(screen.getAllByRole("combobox", { name: "模型角色" })[5]);
    await user.click(await screen.findByRole("option", { name: "Opus" }));
    fireEvent.click(screen.getAllByRole("button", { name: "Delete" })[1]);
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(view.onSubmit).toHaveBeenCalledOnce());
    const mappings = view.onSubmit.mock.calls[0][0].meta
      .claudeDesktopModelRoutes as Record<string, { model: string }>;
    expect(Object.keys(mappings)).toHaveLength(5);
    expect(mappings["claude-opus-5-r2"].model).toBe("upstream-5");
    expect(Object.values(mappings).map((row) => row.model)).toEqual([
      "upstream-0",
      "upstream-2",
      "upstream-3",
      "upstream-4",
      "upstream-5",
    ]);
  });

  describe("importing all models", () => {
    const initialData = {
      name: "Import fixture",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://fixture.example/v1",
          ANTHROPIC_AUTH_TOKEN: "synthetic-key",
        },
      },
      meta: {
        claudeDesktopMode: "proxy" as const,
        claudeDesktopModelRoutes: {
          "claude-sonnet-5": {
            model: "existing-model",
            labelOverride: "Custom label",
            supports1m: true,
          },
          "claude-haiku-4-5": { model: "", supports1m: true },
        },
      },
    };
    const catalog = Array.from({ length: 7 }, (_, index) => ({
      id: `catalog-model-${index}`,
      ownedBy: null,
    }));

    it("fetches and imports the full catalog while preserving existing mappings and skipping duplicates", async () => {
      vi.mocked(fetchModelsForConfig).mockResolvedValue([
        ...catalog,
        { id: " catalog-model-0 ", ownedBy: null },
        { id: " ", ownedBy: null },
      ]);
      const view = renderForm(initialData);
      fireEvent.click(screen.getByRole("button", { name: "一键导入所有模型" }));
      await waitFor(() =>
        expect(
          screen.getAllByRole("textbox", { name: "实际请求模型" }),
        ).toHaveLength(8),
      );
      expect(screen.getByDisplayValue("Custom label")).toBeInTheDocument();
      vi.mocked(fetchModelsForConfig).mockClear();
      fireEvent.click(screen.getByRole("button", { name: "一键导入所有模型" }));
      expect(fetchModelsForConfig).not.toHaveBeenCalled();
      expect(
        screen.getAllByRole("textbox", { name: "实际请求模型" }),
      ).toHaveLength(8);
      fireEvent.click(screen.getByRole("button", { name: "保存" }));
      await waitFor(() => expect(view.onSubmit).toHaveBeenCalledOnce());
      const mappings = view.onSubmit.mock.calls[0][0].meta
        .claudeDesktopModelRoutes as Record<
        string,
        { model: string; labelOverride: string; supports1m?: boolean }
      >;
      expect(
        Object.values(mappings)
          .map((row) => row.model)
          .sort(),
      ).toEqual([...catalog.map(({ id }) => id), "existing-model"].sort());
      expect(mappings["claude-sonnet-5"]).toEqual(
        initialData.meta.claudeDesktopModelRoutes["claude-sonnet-5"],
      );
      expect(mappings["claude-haiku-4-5"]).toMatchObject({
        model: "catalog-model-0",
        labelOverride: "catalog-model-0",
      });
      expect(mappings["claude-haiku-4-5"].supports1m).toBeUndefined();
      expect(
        Object.keys(mappings).every((route) =>
          /^claude-(sonnet|opus|fable|haiku)-/.test(route),
        ),
      ).toBe(true);
    });

    it("reuses a fetched catalog instead of requesting it again", async () => {
      vi.mocked(fetchModelsForConfig).mockResolvedValue(catalog);
      renderForm(initialData);
      fireEvent.click(screen.getByRole("button", { name: "获取模型" }));
      await waitFor(() =>
        expect(screen.getByRole("button", { name: "获取模型" })).toBeEnabled(),
      );
      vi.mocked(fetchModelsForConfig).mockClear();
      fireEvent.click(screen.getByRole("button", { name: "一键导入所有模型" }));
      await waitFor(() =>
        expect(
          screen.getAllByRole("textbox", { name: "实际请求模型" }),
        ).toHaveLength(8),
      );
      expect(fetchModelsForConfig).not.toHaveBeenCalled();
    });

    it("preserves mappings after a failed fetch and allows retrying the import", async () => {
      vi.mocked(fetchModelsForConfig).mockRejectedValueOnce(
        new Error("synthetic fetch failure"),
      );
      renderForm(initialData);
      fireEvent.click(screen.getByRole("button", { name: "一键导入所有模型" }));
      await waitFor(() =>
        expect(
          screen.getByRole("button", { name: "一键导入所有模型" }),
        ).toBeEnabled(),
      );
      expect(screen.getByDisplayValue("existing-model")).toBeInTheDocument();
      expect(screen.getByDisplayValue("Custom label")).toBeInTheDocument();
      expect(
        screen.getAllByRole("textbox", { name: "实际请求模型" }),
      ).toHaveLength(2);
      vi.mocked(fetchModelsForConfig).mockResolvedValue(catalog);
      fireEvent.click(screen.getByRole("button", { name: "一键导入所有模型" }));
      await waitFor(() =>
        expect(
          screen.getAllByRole("textbox", { name: "实际请求模型" }),
        ).toHaveLength(8),
      );
    });

    it("discards an import response after the endpoint changes", async () => {
      let resolve!: (models: typeof catalog) => void;
      vi.mocked(fetchModelsForConfig).mockReturnValueOnce(
        new Promise((done) => {
          resolve = done;
        }),
      );
      const view = renderForm(initialData);
      fireEvent.click(screen.getByRole("button", { name: "一键导入所有模型" }));
      fireEvent.change(view.container.querySelector("#baseUrl")!, {
        target: { value: "https://other.example/v1" },
      });
      await act(async () => {
        resolve(catalog);
      });
      expect(
        screen.getAllByRole("textbox", { name: "实际请求模型" }),
      ).toHaveLength(2);
      expect(
        screen.queryByDisplayValue("catalog-model-0"),
      ).not.toBeInTheDocument();
      expect(screen.getByDisplayValue("Custom label")).toBeInTheDocument();
    });
  });

  it.each(["mapping", "detection"])(
    "applies only the mapping dropdown to the saved model and display name (%s)",
    async (picker) => {
      Element.prototype.scrollIntoView = vi.fn();
      const user = userEvent.setup();
      vi.mocked(fetchModelsForConfig).mockResolvedValue([
        { id: "old-model", ownedBy: null },
        { id: "new-model", ownedBy: null },
      ]);
      const view = renderForm({
        name: "Dropdown fixture",
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://fixture.example/v1",
            ANTHROPIC_AUTH_TOKEN: "synthetic-key",
          },
        },
        meta: {
          claudeDesktopMode: "proxy",
          claudeDesktopModelRoutes: {
            "claude-sonnet-5": {
              model: "old-model",
              labelOverride: "Old label",
            },
          },
        },
      });
      let scope: HTMLElement;
      if (picker === "mapping") {
        fireEvent.click(screen.getByRole("button", { name: "获取模型" }));
        scope = screen.getByDisplayValue("Old label").parentElement!;
      } else {
        scope = view.container.querySelector("section")!;
      }
      const trigger = await within(scope).findByRole("button", {
        name: picker === "mapping" ? "Select model" : "Select detection model",
      });
      await user.click(trigger);
      await user.click(
        await screen.findByRole("option", { name: "new-model" }),
      );
      expect(screen.getByRole("textbox", { name: "菜单显示名" })).toHaveValue(
        picker === "mapping" ? "new-model" : "Old label",
      );
      expect(screen.getByRole("textbox", { name: "实际请求模型" })).toHaveValue(
        picker === "mapping" ? "new-model" : "old-model",
      );
    },
  );

  it("代理模式初始无路由且默认路由未就绪时不渲染空四档", () => {
    // The default-route query is still empty, so leave the list ready for seeding.
    renderForm({
      name: "Proxy Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://api.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-test",
        },
      },
      meta: {
        claudeDesktopMode: "proxy",
        claudeDesktopModelRoutes: {},
      },
    });

    expect(screen.queryAllByPlaceholderText("DeepSeek V4 Pro")).toHaveLength(0);
  });

  it("saves only explicitly configured proxy tiers", async () => {
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Proxy Provider",
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://api.example.com",
            ANTHROPIC_AUTH_TOKEN: "sk-test",
          },
        },
        meta: {
          claudeDesktopMode: "proxy",
          claudeDesktopModelRoutes: {
            "claude-old": {
              model: "upstream-old",
            },
          },
        },
      },
      onSubmit,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    const submitted = onSubmit.mock.calls[0][0];
    expect(submitted.meta.claudeDesktopModelRoutes).toEqual({
      "claude-sonnet-5": {
        model: "upstream-old",
        labelOverride: "upstream-old",
        supports1m: undefined,
      },
    });
  });

  it("does not create additional menu entries from a tier's 1M declaration", async () => {
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Proxy Provider",
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://api.example.com",
            ANTHROPIC_AUTH_TOKEN: "sk-test",
          },
        },
        meta: {
          claudeDesktopMode: "proxy",
          claudeDesktopModelRoutes: {
            "claude-sonnet-4-6": { model: "deepseek-v4-pro", supports1m: true },
          },
        },
      },
      onSubmit,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    const routes = onSubmit.mock.calls[0][0].meta.claudeDesktopModelRoutes;
    expect(routes["claude-sonnet-4-6"]).toMatchObject({
      model: "deepseek-v4-pro",
      supports1m: true,
    });
    expect(Object.keys(routes)).toEqual(["claude-sonnet-4-6"]);
  });

  it("keeps three menu mappings after clearing the fourth model", async () => {
    const { onSubmit } = renderForm({
      name: "Synthetic relay",
      settingsConfig: {
        env: {
          ANTHROPIC_API_KEY: "synthetic-key",
          ANTHROPIC_BASE_URL: "https://example.com",
        },
      },
      meta: {
        claudeDesktopMode: "proxy",
        claudeDesktopModelRoutes: {
          "claude-sonnet-5": { model: "model-a", supports1m: true },
          "claude-opus-5": { model: "model-b", supports1m: true },
          "claude-fable-5": { model: "model-c", supports1m: true },
          "claude-haiku-4-5": {
            model: "model-a",
            labelOverride: "Model A",
            supports1m: true,
          },
        },
      },
    });
    fireEvent.change(screen.getByPlaceholderText("deepseek-v4-flash"), {
      target: { value: "" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    const routes = onSubmit.mock.calls[0][0].meta.claudeDesktopModelRoutes;
    expect(Object.keys(routes)).toEqual([
      "claude-sonnet-5",
      "claude-opus-5",
      "claude-fable-5",
    ]);
    expect(Object.values(routes).every((route: any) => route.supports1m)).toBe(
      true,
    );
  });

  it("保存直连模型列表时不会保留旧 route 作为隐藏映射目标", async () => {
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Direct Provider",
        settingsConfig: {
          env: {
            ANTHROPIC_BASE_URL: "https://api.example.com",
            ANTHROPIC_AUTH_TOKEN: "sk-test",
          },
        },
        meta: {
          claudeDesktopMode: "direct",
          claudeDesktopModelRoutes: {
            "claude-old": {
              model: "claude-old",
            },
          },
        },
      },
      onSubmit,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    const submitted = onSubmit.mock.calls[0][0];
    expect(submitted.meta.claudeDesktopModelRoutes).toMatchObject({
      "claude-sonnet-5": {
        model: "claude-old",
      },
    });
  });

  it("不允许保存需要重新登录的 Codex OAuth 账号", async () => {
    authState.codexReauthRequired = true;
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Codex OAuth Provider",
        category: "third_party",
        settingsConfig: { env: {} },
        meta: {
          providerType: "codex_oauth",
          authBinding: {
            source: "managed_account",
            authProvider: "codex_oauth",
            accountId: "acct-managed",
          },
          claudeDesktopMode: "proxy",
          claudeDesktopModelRoutes: {
            "claude-sonnet-5": { model: "upstream-model" },
          },
        },
      },
      onSubmit,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(
        "已绑定账号不存在或需要重新登录，请重新选择账号",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("未选择账号时不允许保存需要重新登录的 Codex OAuth 默认账号", async () => {
    authState.codexReauthRequired = true;
    const onSubmit = vi.fn();
    renderForm(
      {
        name: "Codex OAuth Default Account Provider",
        category: "third_party",
        settingsConfig: { env: {} },
        meta: {
          providerType: "codex_oauth",
          claudeDesktopMode: "proxy",
          claudeDesktopModelRoutes: {
            "claude-sonnet-5": { model: "upstream-model" },
          },
        },
      },
      onSubmit,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(
        "已绑定账号不存在或需要重新登录，请重新选择账号",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
