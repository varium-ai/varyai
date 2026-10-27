import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { Form } from "@/components/ui/form";
import {
  fetchModelsForConfig,
  fetchXaiOauthModels,
  showFetchModelsError,
} from "@/lib/api/model-fetch";

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: vi.fn(),
  fetchXaiOauthModels: vi.fn(),
  showFetchModelsError: vi.fn(),
}));
vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => <div />,
}));

type Props = ComponentProps<typeof CodexFormFields>;
const catalog = Array.from({ length: 15 }, (_, i) => ({
  id: `fixture-model-${i}`,
  ownedBy: null,
}));

function FormShell({ children }: PropsWithChildren) {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
}

function renderForm(overrides: Partial<Props> = {}) {
  const props: Props = {
    codexApiKey: "fixture-key",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    shouldShowSpeedTest: false,
    codexBaseUrl: "https://fixture.example/v1",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "existing-default",
    onModelChange: vi.fn(),
    apiFormat: "openai_responses",
    onApiFormatChange: vi.fn(),
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    onAnthropicAuthFieldChange: vi.fn(),
    impersonateClaudeCode: false,
    onImpersonateClaudeCodeChange: vi.fn(),
    maxOutputTokens: "",
    onMaxOutputTokensChange: vi.fn(),
    promptCacheRouting: "auto",
    onPromptCacheRoutingChange: vi.fn(),
    catalogModels: [],
    onCatalogModelsChange: vi.fn(),
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    ...overrides,
  };
  return {
    props,
    ...render(
      <FormShell>
        <CodexFormFields {...props} />
      </FormShell>,
    ),
  };
}

describe("Codex model mapping actions", () => {
  it.each(["", "old-model", "Custom label"])(
    "updates an automatic model label while preserving %j",
    async (displayName) => {
      const { props } = renderForm({
        catalogModels: [{ model: "old-model", displayName }],
      });
      fireEvent.click(
        screen.getByRole("button", { name: "providerForm.fetchModels" }),
      );
      await waitFor(() =>
        expect(
          screen.getAllByRole("button", { name: "Select model" }),
        ).toHaveLength(2),
      );
      fireEvent.click(
        screen.getAllByRole("button", { name: "Select model" })[1],
      );
      fireEvent.click(
        await screen.findByRole("option", { name: catalog[0].id }),
      );
      await waitFor(() =>
        expect(props.onCatalogModelsChange).toHaveBeenLastCalledWith([
          expect.objectContaining({
            model: catalog[0].id,
            displayName:
              displayName === "Custom label" ? displayName : catalog[0].id,
          }),
        ]),
      );
    },
  );

  beforeEach(() => {
    HTMLElement.prototype.scrollIntoView = vi.fn();
    vi.mocked(fetchModelsForConfig).mockReset().mockResolvedValue(catalog);
    vi.mocked(fetchXaiOauthModels).mockReset().mockResolvedValue(catalog);
  });

  it("fetches candidates without changing mappings or the default model", async () => {
    const { props } = renderForm({ codexModel: "" });
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledOnce());
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "一键设置" })).toBeEnabled(),
    );
    expect(props.onCatalogModelsChange).not.toHaveBeenCalled();
    expect(props.onModelChange).not.toHaveBeenCalled();
    expect(
      screen.queryAllByRole("textbox", { name: "实际请求模型" }),
    ).toHaveLength(0);
    expect(screen.getByRole("button", { name: "Select model" })).toBeVisible();
  });

  it("imports every fetched model once and preserves custom row settings", async () => {
    const existing = {
      model: catalog[0].id,
      displayName: "Custom name",
      contextWindow: 128000,
      reasoningLevels: ["high", "max"],
      defaultReasoningLevel: "high",
      supportsParallelToolCalls: true,
      inputModalities: ["text", "image"],
    };
    const { props } = renderForm({ catalogModels: [existing] });
    fireEvent.click(screen.getByRole("button", { name: "一键设置" }));
    await waitFor(() =>
      expect(
        screen.getAllByRole("textbox", { name: "实际请求模型" }),
      ).toHaveLength(15),
    );
    expect(props.onCatalogModelsChange).toHaveBeenLastCalledWith([
      existing,
      ...catalog
        .slice(1)
        .map(({ id }) => ({ model: id, displayName: id, contextWindow: "" })),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "一键设置" }));
    expect(
      screen.getAllByRole("textbox", { name: "实际请求模型" }),
    ).toHaveLength(15);
    expect(fetchModelsForConfig).toHaveBeenCalledOnce();
    expect(props.onModelChange).not.toHaveBeenCalled();
  });

  it("keeps mappings intact after a failed import and permits retry", async () => {
    vi.mocked(fetchModelsForConfig).mockRejectedValueOnce(
      new Error("fixture failure"),
    );
    const { props } = renderForm();
    fireEvent.click(screen.getByRole("button", { name: "一键设置" }));
    await waitFor(() => expect(showFetchModelsError).toHaveBeenCalledOnce());
    expect(props.onCatalogModelsChange).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "一键设置" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "一键设置" }));
    await waitFor(() =>
      expect(
        screen.getAllByRole("textbox", { name: "实际请求模型" }),
      ).toHaveLength(15),
    );
  });

  it("does not import stale results after credentials change", async () => {
    let resolve!: (models: typeof catalog) => void;
    vi.mocked(fetchModelsForConfig).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const view = renderForm();
    fireEvent.click(screen.getByRole("button", { name: "一键设置" }));
    view.rerender(
      <FormShell>
        <CodexFormFields {...view.props} codexApiKey="new-fixture-key" />
      </FormShell>,
    );
    resolve(catalog);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "一键设置" })).toBeEnabled(),
    );
    expect(view.props.onCatalogModelsChange).not.toHaveBeenCalled();
    expect(view.props.onModelChange).not.toHaveBeenCalled();
  });

  it("uses the selected managed account for the same import action", async () => {
    renderForm({
      isXaiOauthPreset: true,
      isXaiOauthAuthenticated: true,
      selectedXaiAccountId: "fixture-account",
    });
    fireEvent.click(screen.getByRole("button", { name: "高级选项" }));
    fireEvent.click(screen.getByRole("button", { name: "一键设置" }));
    await waitFor(() =>
      expect(
        screen.getAllByRole("textbox", { name: "实际请求模型" }),
      ).toHaveLength(15),
    );
    expect(fetchXaiOauthModels).toHaveBeenCalledWith("fixture-account");
    expect(fetchModelsForConfig).not.toHaveBeenCalled();
  });
});
