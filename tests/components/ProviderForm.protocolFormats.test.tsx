import { render, screen, waitFor } from "@testing-library/react";
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

vi.mock("@/lib/query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/query")>()),
  useSettingsQuery: () => ({ data: { commonConfigConfirmed: true } }),
}));

describe("provider upstream protocol selection", () => {
  beforeAll(() => {
    Element.prototype.scrollIntoView = vi.fn();
    i18n.addResourceBundle("zh", "translation", zh, true, true);
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
    expect(saved.config).toContain('model = "existing-default"');
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
    expect(saved.env).toEqual(initialData.settingsConfig.env);
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
    "connects explicit detection to the %s form and preserves its saved choice",
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
      expect(screen.getByLabelText("上游格式")).toHaveTextContent(
        "Gemini Native",
      );
      await user.click(screen.getByRole("button", { name: "使用此格式" }));
      expect(screen.getByLabelText("上游格式")).toHaveTextContent(
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

    expect(screen.getByLabelText("上游格式")).toHaveTextContent(
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

      await user.click(screen.getByLabelText("上游格式"));
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
