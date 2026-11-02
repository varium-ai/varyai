import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
import { detectProviderProtocols } from "@/lib/api/provider-protocol-detection";

vi.mock("@/lib/api/provider-protocol-detection", () => ({
  detectProviderProtocols: vi.fn(),
}));
vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/model-fetch")>()),
  fetchModelsForConfig: vi.fn().mockResolvedValue([]),
}));

vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: () => <div />,
}));

vi.mock("@/components/JsonEditor", () => ({ default: () => <div /> }));
vi.mock("@/hooks/useHermes", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/hooks/useHermes")>()),
  useHermesLiveProviderIds: () => ({ data: [], isLoading: false }),
}));

vi.mock("@/lib/query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/query")>()),
  useSettingsQuery: () => ({ data: { commonConfigConfirmed: true } }),
}));

describe("provider upstream protocol selection", () => {
  beforeAll(() => {
    Element.prototype.scrollIntoView = vi.fn();
    i18n.addResourceBundle("zh", "translation", zh, true, true);
  });

  it.each([
    ["claude", "baseUrl", "apiKey"],
    ["claude-desktop", "baseUrl", "apiKey"],
    ["codex", "codexBaseUrl", "codexApiKey"],
    ["gemini", "baseUrl", "apiKey"],
    ["grokbuild", "codexBaseUrl", "codexApiKey"],
    ["opencode", "opencode-baseurl", "apiKey"],
    ["openclaw", "openclaw-baseurl", "apiKey"],
    ["hermes", "hermes-baseurl", "apiKey"],
    ["pi", "pi-provider-base-url", "pi-api-key"],
    ["workbuddy", "workbuddy-url", "workbuddy-api-key"],
    ["qoder", "workbuddy-url", "workbuddy-api-key"],
    ["trae", "agent-base", "agent-key"],
    ["kimi", "agent-base", "agent-key"],
    ["qwen", "agent-base", "agent-key"],
    ["zcode", "agent-base", "agent-key"],
  ] as const)(
    "automatically selects a verified upstream when adding a %s provider",
    async (appId, baseId, keyId) => {
      vi.mocked(fetchModelsForConfig)
        .mockClear()
        .mockResolvedValue([{ id: "synthetic-model", ownedBy: null }]);
      vi.mocked(detectProviderProtocols)
        .mockClear()
        .mockResolvedValue([
          {
            format: appId === "gemini" ? "openai_chat" : "gemini_native",
            text: { status: "verified" },
            stream: { status: "verified" },
          },
        ]);
      const view = render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId={appId}
            submitLabel="Save automatic protocol"
            onSubmit={vi.fn()}
            onCancel={vi.fn()}
          />
        </QueryClientProvider>,
      );
      const base = view.container.querySelector(`#${baseId}`);
      const key = view.container.querySelector(`#${keyId}`);
      expect(base).not.toBeNull();
      expect(key).not.toBeNull();
      fireEvent.change(base!, {
        target: { value: "https://fixture.example/v1" },
      });
      fireEvent.change(key!, { target: { value: "synthetic-key" } });
      await waitFor(
        () => {
          expect(detectProviderProtocols).toHaveBeenCalledWith(
            expect.objectContaining({
              baseUrl: "https://fixture.example/v1",
              apiKey: "synthetic-key",
            }),
          );
          expect(
            screen
              .getAllByRole("combobox")
              .some((element) =>
                element.textContent?.includes(
                  appId === "gemini"
                    ? "OpenAI Chat Completions"
                    : "Gemini Native",
                ),
              ),
          ).toBe(true);
        },
        { timeout: 3500 },
      );
      expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);
      const fetchLabel = ["trae", "kimi", "qwen", "zcode"].includes(appId)
        ? zh.managedAgent.fetchModels
        : zh.providerForm.fetchModels;
      await act(async () => {
        fireEvent.click(screen.getAllByRole("button", { name: fetchLabel })[0]);
      });
      expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);
      vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
      vi.mocked(detectProviderProtocols).mockResolvedValue([]);
    },
  );

  it.each(["qoder", "workbuddy", "hermes"] as const)(
    "saves a Gemini upstream separately from the %s client protocol",
    async (appId) => {
      const user = userEvent.setup();
      const onSubmit = vi.fn();
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId={appId}
            providerId="fixture-provider"
            submitLabel="Save protocol"
            onSubmit={onSubmit}
            onCancel={vi.fn()}
            initialData={{
              name: "Gemini fixture",
              category: "custom",
              settingsConfig:
                appId === "hermes"
                  ? {
                      base_url: "https://models.example/v1beta",
                      api_key: "synthetic-key",
                      api_mode: "chat_completions",
                      models: [{ id: "fixture" }],
                    }
                  : {
                      baseUrl: "https://models.example/v1beta",
                      url: "https://models.example/v1beta",
                      apiKey: "synthetic-key",
                      protocol: "openai",
                      model: "fixture",
                      id: "fixture",
                      models: [{ id: "fixture", model: "fixture" }],
                    },
            }}
          />
        </QueryClientProvider>,
      );
      await user.click(screen.getByLabelText("上游协议"));
      await user.click(screen.getByRole("option", { name: /Gemini Native/ }));
      await user.click(screen.getByRole("button", { name: "Save protocol" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
      const saved = onSubmit.mock.calls[0][0];
      expect(saved.meta.apiFormat).toBe("gemini_native");
      const config = JSON.parse(saved.settingsConfig);
      if (appId === "hermes") expect(config.api_mode).toBe("chat_completions");
      else if (appId === "qoder") expect(config.protocol).toBe("openai");
    },
  );

  it.each([
    ["claude-desktop", "missing-model"],
    ["claude-desktop", "keep-model"],
    ["workbuddy", "missing-model"],
    ["workbuddy", "keep-model"],
    ["qoder", "missing-model"],
    ["qoder", "keep-model"],
  ] as const)(
    "validates %s model %s after explicitly fetching all models",
    async (appId, currentModel) => {
      vi.mocked(fetchModelsForConfig).mockResolvedValue([
        { id: "available-model", ownedBy: null },
        { id: "keep-model", ownedBy: null },
      ]);
      const onSubmit = vi.fn();
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId={appId}
            submitLabel="Save fetched default"
            onSubmit={onSubmit}
            onCancel={vi.fn()}
            initialData={{
              name: "Fixture",
              category: "custom",
              settingsConfig:
                appId === "claude-desktop"
                  ? {
                      env: {
                        ANTHROPIC_BASE_URL: "https://fixture.example/v1",
                        ANTHROPIC_AUTH_TOKEN: "synthetic-key",
                      },
                    }
                  : {
                      baseUrl: "https://fixture.example/v1",
                      url: "https://fixture.example/v1",
                      apiKey: "synthetic-key",
                      protocol: "openai",
                      id: currentModel,
                      model: currentModel,
                      models: [{ id: currentModel, model: currentModel }],
                    },
              meta: {
                apiFormat: "openai_chat",
                claudeDesktopMode: "proxy",
                claudeDesktopModelRoutes: {
                  "claude-sonnet-4-6": { model: currentModel },
                },
              },
            }}
          />
        </QueryClientProvider>,
      );
      fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));
      await waitFor(() =>
        expect(
          screen.getByRole("button", { name: "获取模型列表" }),
        ).toBeEnabled(),
      );
      fireEvent.click(
        screen.getByRole("button", { name: "Save fetched default" }),
      );
      await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
      const saved = onSubmit.mock.calls[0][0];
      const expected =
        currentModel === "keep-model" ? currentModel : "available-model";
      if (appId === "claude-desktop") {
        const routes = Object.values(saved.meta.claudeDesktopModelRoutes) as {
          model: string;
        }[];
        expect(routes.length).toBeGreaterThan(0);
        expect(routes.some((row) => row.model === expected)).toBe(true);
        expect(
          routes.every((row) =>
            ["available-model", "keep-model"].includes(row.model),
          ),
        ).toBe(true);
      } else {
        const config = JSON.parse(saved.settingsConfig);
        expect(appId === "qoder" ? config.model : config.id).toBe(expected);
      }
      vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
    },
  );

  it("does not change Desktop mappings from an obsolete model response", async () => {
    let resolve!: (models: { id: string; ownedBy: null }[]) => void;
    vi.mocked(fetchModelsForConfig).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="claude-desktop"
          submitLabel="Save unchanged mapping"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "Fixture",
            category: "custom",
            settingsConfig: {
              env: {
                ANTHROPIC_BASE_URL: "https://fixture.example/v1",
                ANTHROPIC_AUTH_TOKEN: "synthetic-key",
              },
            },
            meta: {
              apiFormat: "openai_chat",
              claudeDesktopMode: "proxy",
              claudeDesktopModelRoutes: {
                "claude-sonnet-4-6": { model: "original-model" },
              },
            },
          }}
        />
      </QueryClientProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "changed-synthetic-key" },
    });
    await act(async () => {
      resolve([{ id: "obsolete-model", ownedBy: null }]);
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Save unchanged mapping" }),
    );
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    const routes = Object.values(
      onSubmit.mock.calls[0][0].meta.claudeDesktopModelRoutes,
    ) as { model: string }[];
    expect(routes.some((row) => row.model === "original-model")).toBe(true);
    expect(routes.some((row) => row.model === "obsolete-model")).toBe(false);
  });

  it("saves and reopens all Codex models after fetching candidates separately", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const catalog = Array.from({ length: 15 }, (_, i) => ({
      id: `fixture-model-${i}`,
      ownedBy: null,
    }));
    vi.mocked(fetchModelsForConfig).mockResolvedValue(catalog);
    const initialData = {
      name: "Codex import fixture",
      category: "third_party" as const,
      settingsConfig: {
        auth: { OPENAI_API_KEY: "fixture-key" },
        config:
          'model = "existing-default"\nmodel_provider = "custom"\n[model_providers.custom]\nname = "Relay"\nbase_url = "https://relay.example/v1"\nwire_api = "responses"\n',
      },
    };
    const view = render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="codex"
          providerId="fixture-codex-import"
          submitLabel="Save imported models"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={initialData}
        />
      </QueryClientProvider>,
    );
    await user.click(screen.getByRole("button", { name: "获取模型列表" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "一键设置" })).toBeEnabled(),
    );
    expect(
      screen.queryAllByRole("textbox", { name: "实际请求模型" }),
    ).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "一键设置" }));
    await waitFor(() =>
      expect(
        screen.getAllByRole("textbox", { name: "实际请求模型" }),
      ).toHaveLength(15),
    );
    await user.click(
      screen.getByRole("button", { name: "Save imported models" }),
    );
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    const saved = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(saved.modelCatalog.models).toEqual(
      catalog.map(({ id }) => ({ model: id, displayName: id })),
    );
    expect(saved.config).toContain('model = "fixture-model-0"');
    expect(saved.auth).toEqual(initialData.settingsConfig.auth);
    view.unmount();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="codex"
          providerId="fixture-codex-import"
          submitLabel="Save imported models"
          onSubmit={vi.fn()}
          onCancel={vi.fn()}
          initialData={{ ...initialData, settingsConfig: saved }}
        />
      </QueryClientProvider>,
    );
    expect(
      screen
        .getAllByRole("textbox", { name: "实际请求模型" })
        .map((input) => (input as HTMLInputElement).value),
    ).toEqual(catalog.map(({ id }) => id));
    vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
  });

  it.each(["existing-default", "keep-model"])(
    "imports Gemini candidates and validates the default %s against the supplier",
    async (currentDefault) => {
      const user = userEvent.setup();
      const onSubmit = vi.fn();
      vi.mocked(fetchModelsForConfig).mockResolvedValue([
        { id: "new-a", ownedBy: null },
        { id: "new-b", ownedBy: null },
        { id: "new-a", ownedBy: null },
        { id: " ", ownedBy: null },
        { id: "keep-model", ownedBy: null },
      ]);
      const initialData = {
        name: "Gemini import fixture",
        category: "third_party" as const,
        settingsConfig: {
          env: {
            GOOGLE_GEMINI_BASE_URL: "https://relay.example/v1beta",
            GEMINI_API_KEY: "synthetic-key",
            GEMINI_MODEL: currentDefault,
          },
          config: {
            experimental: { keepFlag: true },
            mcpServers: { fixture: { command: "synthetic-mcp" } },
            modelConfigs: {
              aliases: { fixture: { model: "keep-model" } },
              modelDefinitions: {
                "keep-model": {
                  displayName: "Keep label",
                  isVisible: true,
                  features: { thinking: true },
                },
              },
            },
          },
        },
      };
      const view = render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId="gemini"
            providerId="fixture-gemini-import"
            submitLabel="Save Gemini models"
            onSubmit={onSubmit}
            onCancel={vi.fn()}
            initialData={initialData}
          />
        </QueryClientProvider>,
      );
      await user.click(screen.getByRole("button", { name: "获取模型列表" }));
      await waitFor(() =>
        expect(screen.getByRole("button", { name: "一键设置" })).toBeEnabled(),
      );
      expect(
        screen.getAllByRole("textbox", { name: "实际请求模型" }),
      ).toHaveLength(1);
      expect(view.container.querySelector("#gemini-model")).toHaveValue(
        currentDefault === "keep-model" ? currentDefault : "new-a",
      );
      await user.click(screen.getByRole("button", { name: "一键设置" }));
      await user.click(screen.getByRole("button", { name: "一键设置" }));
      expect(
        screen.getAllByRole("textbox", { name: "实际请求模型" }),
      ).toHaveLength(3);
      await user.click(
        screen.getByRole("button", { name: "Save Gemini models" }),
      );
      await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
      const saved = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
      expect(saved.env).toEqual({
        ...initialData.settingsConfig.env,
        GEMINI_MODEL:
          currentDefault === "keep-model" ? currentDefault : "new-a",
      });
      expect(saved.config.experimental).toEqual({
        keepFlag: true,
        dynamicModelConfiguration: true,
      });
      expect(saved.config.mcpServers).toEqual(
        initialData.settingsConfig.config.mcpServers,
      );
      expect(saved.config.modelConfigs.aliases).toEqual(
        initialData.settingsConfig.config.modelConfigs.aliases,
      );
      expect(saved.config.modelConfigs.modelDefinitions).toEqual({
        ...initialData.settingsConfig.config.modelConfigs.modelDefinitions,
        "new-a": { displayName: "new-a", isVisible: true },
        "new-b": { displayName: "new-b", isVisible: true },
      });
      view.unmount();
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId="gemini"
            providerId="fixture-gemini-import"
            submitLabel="Save Gemini models"
            onSubmit={vi.fn()}
            onCancel={vi.fn()}
            initialData={{ ...initialData, settingsConfig: saved }}
          />
        </QueryClientProvider>,
      );
      expect(
        screen
          .getAllByRole("textbox", { name: "实际请求模型" })
          .map((input) => (input as HTMLInputElement).value),
      ).toEqual(["keep-model", "new-a", "new-b"]);
      vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
    },
  );

  it("saves and reopens every imported Claude model as a separate row", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const catalog = Array.from({ length: 7 }, (_, i) => ({
      id: `fixture-model-${i}`,
      ownedBy: null,
    }));
    vi.mocked(fetchModelsForConfig).mockResolvedValue(catalog);
    const initialData = {
      name: "Import fixture",
      category: "third_party" as const,
      settingsConfig: {
        env: {
          ANTHROPIC_API_KEY: "fixture-key",
          ANTHROPIC_BASE_URL: "https://relay.example/v1",
          ANTHROPIC_MODEL: "fallback-model",
          ANTHROPIC_DEFAULT_OPUS_MODEL: "fixture-model-1[1m]",
          ANTHROPIC_DEFAULT_HAIKU_MODEL: "missing-haiku",
          CLAUDE_CODE_SUBAGENT_MODEL: "inherit",
        },
      },
    };
    const view = render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="claude"
          providerId="fixture-import"
          submitLabel="Save imported models"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={initialData}
        />
      </QueryClientProvider>,
    );
    await user.click(screen.getByRole("button", { name: "一键设置" }));
    await waitFor(() =>
      expect(
        screen.getAllByRole("textbox", { name: "实际请求模型" }),
      ).toHaveLength(7),
    );
    await user.click(screen.getByRole("button", { name: "一键设置" }));
    expect(
      screen.getAllByRole("textbox", { name: "实际请求模型" }),
    ).toHaveLength(7);
    await user.click(
      screen.getByRole("button", { name: "Save imported models" }),
    );
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    const saved = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(saved.modelPicker.options).toEqual(
      catalog.map(({ id }) => ({ model: id, label: id })),
    );
    expect(saved.env).toMatchObject({
      ...initialData.settingsConfig.env,
      ANTHROPIC_MODEL: "fixture-model-0",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "fixture-model-0",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "fixture-model-0",
    });
    expect(saved.env).not.toHaveProperty("ANTHROPIC_DEFAULT_FABLE_MODEL");
    view.unmount();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="claude"
          providerId="fixture-import"
          submitLabel="Save imported models"
          onSubmit={vi.fn()}
          onCancel={vi.fn()}
          initialData={{ ...initialData, settingsConfig: saved }}
        />
      </QueryClientProvider>,
    );
    expect(
      screen
        .getAllByRole("textbox", { name: "实际请求模型" })
        .map((field) => (field as HTMLInputElement).value),
    ).toEqual(catalog.map(({ id }) => id));
    vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
  });

  it.each(["claude", "codex", "gemini"] as const)(
    "applies the first verified protocol after explicit detection in the %s form",
    async (appId) => {
      const user = userEvent.setup();
      vi.mocked(detectProviderProtocols).mockResolvedValue([
        {
          format: "openai_chat",
          text: { status: "verified" },
          stream: { status: "verified" },
        },
      ]);
      const settingsConfig =
        appId === "claude"
          ? {
              env: {
                ANTHROPIC_API_KEY: "fixture-key",
                ANTHROPIC_BASE_URL: "https://relay.example/v1",
                ANTHROPIC_MODEL: "fixture-model",
              },
            }
          : appId === "gemini"
            ? {
                env: {
                  GEMINI_API_KEY: "fixture-key",
                  GOOGLE_GEMINI_BASE_URL: "https://relay.example/v1",
                  GEMINI_MODEL: "fixture-model",
                },
                config: {},
              }
            : {
                auth: { OPENAI_API_KEY: "fixture-key" },
                config:
                  'model = "fixture-model"\nmodel_provider = "custom"\n[model_providers.custom]\nname = "Relay"\nbase_url = "https://relay.example/v1"\nwire_api = "responses"\n',
              };
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId={appId}
            providerId="fixture-provider"
            submitLabel="Save protocol"
            onSubmit={vi.fn()}
            onCancel={vi.fn()}
            initialData={{
              name: "Fixture",
              category: "third_party",
              settingsConfig,
              meta: { apiFormat: "gemini_native" },
            }}
          />
        </QueryClientProvider>,
      );
      vi.mocked(fetchModelsForConfig).mockClear();
      await user.click(screen.getByRole("button", { name: "获取模型列表" }));
      await waitFor(() =>
        expect(vi.mocked(fetchModelsForConfig).mock.calls).toContainEqual([
          "https://relay.example/v1",
          "fixture-key",
          appId === "gemini" ? undefined : false,
          undefined,
          appId === "gemini" ? undefined : "",
          { apiFormat: "google-generative-ai" },
        ]),
      );
      expect(screen.getByLabelText("上游协议")).toHaveTextContent(
        "Gemini Native",
      );
      await user.click(screen.getByRole("button", { name: "检测配置" }));
      await waitFor(() =>
        expect(detectProviderProtocols).toHaveBeenCalledWith(
          expect.objectContaining({
            baseUrl: "https://relay.example/v1",
            apiKey: "fixture-key",
            model: "fixture-model",
          }),
        ),
      );
      expect(screen.getByLabelText("上游协议")).toHaveTextContent(
        "OpenAI Chat Completions",
      );
      vi.mocked(detectProviderProtocols).mockClear();
    },
  );

  it("preserves a Gemini provider's Chat upstream while editing", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="gemini"
          providerId="gemini-chat"
          submitLabel="Save protocol"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "Gemini via Chat",
            category: "third_party",
            settingsConfig: {
              env: {
                GEMINI_API_KEY: "fixture-key",
                GOOGLE_GEMINI_BASE_URL: "https://relay.example/v1",
                GEMINI_MODEL: "fixture-model",
              },
              config: {},
            },
            meta: { apiFormat: "openai_chat" },
          }}
        />
      </QueryClientProvider>,
    );

    expect(screen.getByLabelText("上游协议")).toHaveTextContent(
      "OpenAI Chat Completions",
    );
    await user.click(screen.getByRole("button", { name: "Save protocol" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onSubmit.mock.calls[0][0].meta.apiFormat).toBe("openai_chat");
  });

  it.each(["openai_responses", "gemini_native"] as const)(
    "supports a Gemini upstream for a Codex provider initially using %s",
    async (initialFormat) => {
      const user = userEvent.setup();
      const onSubmit = vi.fn();
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId="codex"
            providerId="codex-gemini"
            submitLabel="Save protocol"
            onSubmit={onSubmit}
            onCancel={vi.fn()}
            initialData={{
              name: "Codex via Gemini",
              category: "third_party",
              settingsConfig: {
                auth: { OPENAI_API_KEY: "fixture-key" },
                config:
                  'model = "fixture-model"\nmodel_provider = "custom"\n[model_providers.custom]\nname = "Relay"\nbase_url = "https://relay.example/v1beta"\nwire_api = "responses"\n',
              },
              meta: { apiFormat: initialFormat },
            }}
          />
        </QueryClientProvider>,
      );

      await user.click(screen.getByLabelText("上游协议"));
      await user.click(screen.getByRole("option", { name: /Gemini Native/ }));
      await user.click(screen.getByRole("button", { name: "Save protocol" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
      const submitted = onSubmit.mock.calls[0][0];
      expect(submitted.meta.apiFormat).toBe("gemini_native");
      expect(JSON.parse(submitted.settingsConfig).config).toContain(
        'wire_api = "responses"',
      );
    },
  );
});
