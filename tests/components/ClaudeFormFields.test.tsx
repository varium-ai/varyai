import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ClaudeFormFields } from "@/components/providers/forms/ClaudeFormFields";
import { Form } from "@/components/ui/form";
import { detectProviderProtocols } from "@/lib/api/provider-protocol-detection";

vi.mock("@/lib/api/provider-protocol-detection", () => ({
  detectProviderProtocols: vi.fn().mockResolvedValue([]),
}));

const copilotApiMock = vi.hoisted(() => ({
  copilotGetModels: vi.fn(),
  copilotGetModelsForAccount: vi.fn(),
}));

const modelFetchApiMock = vi.hoisted(() => ({
  fetchCodexOauthModels: vi.fn(),
  fetchModelsForConfig: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/lib/api/copilot", () => ({
  copilotGetModels: copilotApiMock.copilotGetModels,
  copilotGetModelsForAccount: copilotApiMock.copilotGetModelsForAccount,
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchCodexOauthModels: modelFetchApiMock.fetchCodexOauthModels,
  fetchModelsForConfig: modelFetchApiMock.fetchModelsForConfig,
  showFetchModelsError: modelFetchApiMock.showFetchModelsError,
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div data-testid="copilot-auth-section" />,
}));

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex-oauth-section" />,
}));

type ClaudeFormFieldsProps = ComponentProps<typeof ClaudeFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();

  return <Form {...form}>{children}</Form>;
};

const renderCopilotForm = (overrides: Partial<ClaudeFormFieldsProps> = {}) => {
  const props: ClaudeFormFieldsProps = {
    shouldShowApiKey: false,
    apiKey: "",
    onApiKeyChange: vi.fn(),
    category: "official",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    isCopilotPreset: true,
    usesOAuth: true,
    isCopilotAuthenticated: true,
    selectedGitHubAccountId: "gh-1",
    onGitHubAccountSelect: vi.fn(),
    isCodexOauthPreset: false,
    isCodexOauthAuthenticated: false,
    selectedCodexAccountId: null,
    onCodexAccountSelect: vi.fn(),
    codexFastMode: false,
    onCodexFastModeChange: vi.fn(),
    templateValueEntries: [],
    templateValues: {},
    templatePresetName: "",
    onTemplateValueChange: vi.fn(),
    shouldShowSpeedTest: false,
    baseUrl: "",
    onBaseUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    onCustomEndpointsChange: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    showEndpointTools: true,
    shouldShowModelSelector: true,
    claudeModel: "",
    defaultHaikuModel: "",
    defaultHaikuModelName: "",
    defaultSonnetModel: "claude-sonnet",
    defaultSonnetModelName: "Claude Sonnet",
    defaultOpusModel: "",
    defaultOpusModelName: "",
    defaultFableModel: "",
    defaultFableModelName: "",
    subagentModel: "",
    onModelChange: vi.fn(),
    modelPickerOptions: [],
    onModelPickerOptionsChange: vi.fn(),
    speedTestEndpoints: [],
    apiFormat: "anthropic",
    onApiFormatChange: vi.fn(),
    apiKeyField: "ANTHROPIC_AUTH_TOKEN",
    onApiKeyFieldChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
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
        <ClaudeFormFields {...props} />
      </FormShell>,
    ),
  };
};

const renderCodexOauthForm = (overrides: Partial<ClaudeFormFieldsProps> = {}) =>
  renderCopilotForm({
    isCopilotPreset: false,
    isCopilotAuthenticated: false,
    selectedGitHubAccountId: null,
    isCodexOauthPreset: true,
    isCodexOauthAuthenticated: true,
    selectedCodexAccountId: "chatgpt-1",
    ...overrides,
  });

describe("ClaudeFormFields", () => {
  it("passes the Gemini protocol when fetching models", async () => {
    renderCopilotForm({
      isCopilotPreset: false,
      category: "third_party",
      apiKey: "fixture-key",
      baseUrl: "https://fixture.example/v1",
      apiFormat: "gemini_native",
    });
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() =>
      expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalledWith(
        "https://fixture.example/v1",
        "fixture-key",
        expect.anything(),
        undefined,
        expect.anything(),
        { apiFormat: "google-generative-ai" },
      ),
    );
  });

  it("shares discovered models with every role selector", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([
      { id: "fixture-fast", ownedBy: "fixture" },
      { id: "fixture-large", ownedBy: "fixture" },
    ]);
    const onModelChange = vi.fn();
    renderCopilotForm({
      isCopilotPreset: false,
      category: "third_party",
      apiKey: "fixture-key",
      baseUrl: "https://fixture.example/v1",
      defaultSonnetModel: "",
      defaultSonnetModelName: "",
      onModelChange,
      configurationDetection: {
        editingCredentials: false,
        autoSelectFormat: true,
      },
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.advancedOptionsToggle",
      }),
    );
    await waitFor(
      () =>
        expect(
          screen.getAllByRole("button", { name: "Select model" }),
        ).toHaveLength(7),
      { timeout: 2000 },
    );
    fireEvent.click(screen.getAllByRole("button", { name: "Select model" })[0]);
    expect(screen.getByRole("option", { name: "fixture-fast" })).toBeVisible();
    expect(screen.getByRole("option", { name: "fixture-large" })).toBeVisible();
  });

  it("can detect an explicitly mapped role when the fallback model is empty", async () => {
    renderCopilotForm({
      isCopilotPreset: false,
      category: "third_party",
      apiKey: "fixture-key",
      baseUrl: "https://fixture.example/v1",
      defaultSonnetModel: "fixture-role[1M]",
      claudeModel: "",
      configurationDetection: {
        editingCredentials: false,
        autoSelectFormat: true,
      },
    });
    expect(
      screen.getByRole("button", { name: "providerDetection.check" }),
    ).toBeEnabled();
    expect(
      screen.getByText("providerDetection.selectedModel"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.check" }),
    );
    await waitFor(() =>
      expect(detectProviderProtocols).toHaveBeenCalledWith(
        expect.objectContaining({ model: "fixture-role" }),
      ),
    );
  });

  it("discards a pending manual catalog when credentials change", async () => {
    let resolve!: (models: { id: string; ownedBy: null }[]) => void;
    modelFetchApiMock.fetchModelsForConfig.mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const view = renderCopilotForm({
      isCopilotPreset: false,
      category: "third_party",
      apiKey: "fixture-key",
      baseUrl: "https://fixture.example/v1",
    });
    fireEvent.click(screen.getByRole("button", { name: "一键设置" }));
    view.rerender(
      <FormShell>
        <ClaudeFormFields {...view.props} apiKey="new-fixture-key" />
      </FormShell>,
    );
    resolve([{ id: "old-provider-model", ownedBy: null }]);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "一键设置" })).toBeEnabled(),
    );
    expect(view.props.onModelPickerOptionsChange).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "Select model" }),
    ).not.toBeInTheDocument();
  });

  beforeEach(() => {
    copilotApiMock.copilotGetModels.mockResolvedValue([]);
    copilotApiMock.copilotGetModelsForAccount.mockResolvedValue([]);
    modelFetchApiMock.fetchCodexOauthModels.mockResolvedValue([]);
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([]);
  });

  it("不会在 Copilot 表单打开时自动获取模型列表", () => {
    renderCopilotForm();

    expect(copilotApiMock.copilotGetModels).not.toHaveBeenCalled();
    expect(copilotApiMock.copilotGetModelsForAccount).not.toHaveBeenCalled();
  });

  it("点击获取模型列表后才请求当前 Copilot 账号的模型", async () => {
    renderCopilotForm();

    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.fetchModels",
      }),
    );

    await waitFor(() => {
      expect(copilotApiMock.copilotGetModelsForAccount).toHaveBeenCalledWith(
        "gh-1",
      );
    });
    expect(copilotApiMock.copilotGetModels).not.toHaveBeenCalled();
  });

  it("不会在 Codex OAuth 表单打开时自动获取模型列表", () => {
    renderCodexOauthForm();

    expect(modelFetchApiMock.fetchCodexOauthModels).not.toHaveBeenCalled();
  });

  it("点击获取模型列表后才请求当前 Codex OAuth 账号的模型", async () => {
    renderCodexOauthForm();

    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.fetchModels",
      }),
    );

    await waitFor(() => {
      expect(modelFetchApiMock.fetchCodexOauthModels).toHaveBeenCalledWith(
        "chatgpt-1",
      );
    });
  });

  it("one-click imports every unique model as a row without overwriting roles", async () => {
    const models = Array.from({ length: 7 }, (_, i) => ({
      id: `fixture-${i}`,
      ownedBy: null,
    }));
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([
      ...models,
      models[0],
      { id: "", ownedBy: null },
    ]);
    const onModelChange = vi.fn();
    const onModelPickerOptionsChange = vi.fn();
    const view = renderCopilotForm({
      isCopilotPreset: false,
      category: "third_party",
      apiKey: "fixture-key",
      baseUrl: "https://fixture.example/v1",
      claudeModel: "",
      defaultSonnetModel: "",
      defaultSonnetModelName: "",
      onModelChange,
      onModelPickerOptionsChange,
      modelPickerOptions: [
        { model: "fixture-0", label: "Keep my name", description: "Keep this" },
      ],
    });
    const button = screen.getByRole("button", { name: "一键设置" });
    expect(button).toBeEnabled();
    fireEvent.click(button);
    await waitFor(() =>
      expect(onModelPickerOptionsChange).toHaveBeenCalledTimes(1),
    );
    const imported = onModelPickerOptionsChange.mock.calls[0][0];
    expect(imported).toEqual([
      { model: "fixture-0", label: "Keep my name", description: "Keep this" },
      ...models.slice(1).map(({ id }) => ({ model: id, label: id })),
    ]);
    expect(onModelChange).not.toHaveBeenCalled();
    view.rerender(
      <FormShell>
        <ClaudeFormFields {...view.props} modelPickerOptions={imported} />
      </FormShell>,
    );
    expect(
      screen.getAllByRole("textbox", { name: "Request model" }),
    ).toHaveLength(7);
    fireEvent.click(screen.getByRole("button", { name: "一键设置" }));
    await waitFor(() =>
      expect(onModelPickerOptionsChange).toHaveBeenCalledTimes(2),
    );
    expect(onModelPickerOptionsChange.mock.calls[1][0]).toEqual(imported);
    expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalledTimes(1);
    fireEvent.change(
      screen.getAllByRole("textbox", { name: "Display name" })[0],
      { target: { value: "Renamed" } },
    );
    expect(onModelPickerOptionsChange.mock.lastCall?.[0][0]).toMatchObject({
      model: "fixture-0",
      label: "Renamed",
      description: "Keep this",
    });
    fireEvent.click(screen.getAllByRole("button", { name: "Remove model" })[0]);
    expect(onModelPickerOptionsChange.mock.lastCall?.[0]).toHaveLength(6);
  });

  it("imports from empty role fields and leaves existing rows unchanged on fetch failure", async () => {
    const onModelPickerOptionsChange = vi.fn();
    modelFetchApiMock.fetchModelsForConfig.mockRejectedValueOnce(
      new Error("fixture failure"),
    );
    renderCopilotForm({
      isCopilotPreset: false,
      apiKey: "fixture-key",
      baseUrl: "https://fixture.example/v1",
      claudeModel: "",
      defaultSonnetModel: "",
      defaultSonnetModelName: "",
      onModelPickerOptionsChange,
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.advancedOptionsToggle",
      }),
    );
    const button = screen.getByRole("button", { name: "一键设置" });
    expect(button).toBeEnabled();
    fireEvent.click(button);
    await waitFor(() =>
      expect(modelFetchApiMock.showFetchModelsError).toHaveBeenCalled(),
    );
    expect(onModelPickerOptionsChange).not.toHaveBeenCalled();
    await waitFor(() => expect(button).toBeEnabled());
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValueOnce([
      { id: "fixture-new", ownedBy: null },
    ]);
    fireEvent.click(button);
    await waitFor(() =>
      expect(onModelPickerOptionsChange).toHaveBeenCalledWith([
        { model: "fixture-new", label: "fixture-new" },
      ]),
    );
  });

  it("one-click imports the selected OAuth account's entire catalog", async () => {
    copilotApiMock.copilotGetModelsForAccount.mockResolvedValue([
      { id: "fixture-a" },
      { id: "fixture-b" },
    ]);
    const onModelPickerOptionsChange = vi.fn();
    renderCopilotForm({ onModelPickerOptionsChange });
    fireEvent.click(screen.getByRole("button", { name: "一键设置" }));
    await waitFor(() =>
      expect(onModelPickerOptionsChange).toHaveBeenCalledWith([
        { model: "fixture-a", label: "fixture-a" },
        { model: "fixture-b", label: "fixture-b" },
      ]),
    );
    expect(copilotApiMock.copilotGetModelsForAccount).toHaveBeenCalledWith(
      "gh-1",
    );
  });
});
