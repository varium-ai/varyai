import userEvent from "@testing-library/user-event";
import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { ManagedAgentProviderForm } from "@/components/providers/forms/ManagedAgentProviderForm";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
import { detectProviderProtocols } from "@/lib/api/provider-protocol-detection";

vi.mock("@/lib/api/model-fetch", async (original) => ({
  ...(await original<typeof import("@/lib/api/model-fetch")>()),
  fetchModelsForConfig: vi.fn(),
}));
vi.mock("@/lib/api/provider-protocol-detection", () => ({
  detectProviderProtocols: vi.fn().mockResolvedValue([]),
}));

it("keeps the ZCode probe separate and fills configuration from the discovered catalog", async () => {
  vi.useFakeTimers();
  Element.prototype.scrollIntoView = vi.fn();
  vi.mocked(fetchModelsForConfig)
    .mockClear()
    .mockResolvedValue([
      { id: "configured-model", ownedBy: null },
      { id: "probe-model", ownedBy: null },
    ]);
  const submit = vi.fn();
  try {
    render(
      <ManagedAgentProviderForm
        appId="zcode"
        initialData={{
          name: "Synthetic provider",
          category: "custom",
          settingsConfig: {
            baseUrl: "https://fixture.example/v1",
            apiKey: "synthetic-key",
            models: [{ model: "configured-model" }],
          },
        }}
        onSubmit={submit}
        onCancel={vi.fn()}
        submitLabel="Save"
      />,
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(850);
    });
    const detection = within(
      screen.getByRole("region", { name: "providerDetection.title" }),
    );
    fireEvent.click(
      detection.getByRole("button", { name: "Select detection model" }),
    );
    fireEvent.click(screen.getByRole("option", { name: "probe-model" }));
    expect(screen.getByLabelText("managedAgent.models")).toHaveValue(
      "configured-model",
    );
    await act(async () => {
      fireEvent.click(
        detection.getByRole("button", { name: "providerDetection.check" }),
      );
    });
    expect(detectProviderProtocols).toHaveBeenLastCalledWith(
      expect.objectContaining({ model: "probe-model" }),
    );
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "managedAgent.fetchModels" }),
      );
    });
    expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("managedAgent.models")).toHaveValue(
      "configured-model\nprobe-model",
    );
    vi.useRealTimers();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
  } finally {
    vi.useRealTimers();
  }
});

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
it("discards an old ZCode catalog response after the connection changes", async () => {
  vi.useFakeTimers();
  let resolveOld!: (models: { id: string; ownedBy: null }[]) => void;
  vi.mocked(fetchModelsForConfig)
    .mockClear()
    .mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      }),
    );
  try {
    render(
      <ManagedAgentProviderForm
        appId="zcode"
        initialData={{
          name: "Synthetic provider",
          category: "custom",
          settingsConfig: {
            baseUrl: "https://fixture.example/v1",
            apiKey: "synthetic-key",
            models: [{ model: "configured-model" }],
          },
        }}
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
        submitLabel="Save"
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "managedAgent.fetchModels" }),
    );
    fireEvent.change(screen.getByLabelText("managedAgent.apiKey"), {
      target: { value: "another-synthetic-key" },
    });
    await act(async () => {
      resolveOld([{ id: "stale-model", ownedBy: null }]);
    });
    expect(screen.getByLabelText("managedAgent.models")).toHaveValue(
      "configured-model",
    );
    vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
      { id: "current-model", ownedBy: null },
    ]);
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "managedAgent.fetchModels" }),
      );
    });
    expect(fetchModelsForConfig).toHaveBeenCalledTimes(2);
    expect(screen.getByLabelText("managedAgent.models")).toHaveValue(
      "configured-model\ncurrent-model",
    );
  } finally {
    vi.useRealTimers();
  }
});

it.each(["kimi", "qwen", "zcode", "trae"] as const)(
  "saves a %s multi-model configuration without changing native login",
  async (appId) => {
    const submit = vi.fn();
    const add = vi.fn();
    render(
      <ManagedAgentProviderForm
        appId={appId}
        initialData={{
          name: "Official",
          category: "official",
          settingsConfig: {},
        }}
        onSubmit={submit}
        onAddThirdParty={add}
        onCancel={vi.fn()}
        submitLabel="Save"
      />,
    );
    fireEvent.change(screen.getByLabelText("managedAgent.baseUrl"), {
      target: { value: "https://models.example/v1" },
    });
    fireEvent.change(screen.getByLabelText("managedAgent.apiKey"), {
      target: { value: "synthetic-test-key" },
    });
    fireEvent.change(screen.getByLabelText("managedAgent.models"), {
      target: { value: "alpha\nbeta" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(add).toHaveBeenCalledOnce());
    expect(JSON.parse(add.mock.calls[0][0].settingsConfig)).toMatchObject({
      baseUrl: "https://models.example/v1",
      protocol: "openai",
      models: [{ model: "alpha" }, { model: "beta" }],
    });
    expect(submit).toHaveBeenCalledWith(
      expect.objectContaining({
        presetCategory: "official",
        settingsConfig: "{}",
      }),
    );
  },
);

it("preserves saved per-model capabilities when editing the model list", async () => {
  const submit = vi.fn();
  render(
    <ManagedAgentProviderForm
      appId="kimi"
      initialData={{
        name: "Gateway",
        category: "custom",
        settingsConfig: {
          baseUrl: "https://models.example/v1",
          apiKey: "synthetic-test-key",
          protocol: "openai",
          models: [
            { model: "alpha", contextWindow: 32768, capabilities: ["vision"] },
          ],
        },
      }}
      onSubmit={submit}
      onCancel={vi.fn()}
      submitLabel="Save"
    />,
  );
  fireEvent.change(screen.getByLabelText("managedAgent.models"), {
    target: { value: "alpha\nbeta" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(submit).toHaveBeenCalledOnce());
  expect(JSON.parse(submit.mock.calls[0][0].settingsConfig).models).toEqual([
    { model: "alpha", contextWindow: 32768, capabilities: ["vision"] },
    { model: "beta" },
  ]);
});

it.each(["kimi", "qwen", "zcode", "trae"] as const)(
  "saves Gemini as the %s upstream without inventing a native Gemini protocol",
  async (appId) => {
    const submit = vi.fn();
    render(
      <ManagedAgentProviderForm
        appId={appId}
        initialData={{
          name: "Gateway",
          category: "custom",
          settingsConfig: {
            baseUrl: "https://models.example/v1beta",
            apiKey: "synthetic-key",
            protocol: "openai",
            models: [{ model: "fixture" }],
          },
        }}
        onSubmit={submit}
        onCancel={vi.fn()}
        submitLabel="Save"
      />,
    );
    Element.prototype.scrollIntoView = vi.fn();
    const user = userEvent.setup();
    await user.click(screen.getByLabelText("providerForm.apiFormat"));
    await user.click(
      screen.getByRole("option", {
        name: "providerForm.apiFormatGeminiNative",
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    expect(submit.mock.calls[0][0].meta.apiFormat).toBe("gemini_native");
    expect(JSON.parse(submit.mock.calls[0][0].settingsConfig).protocol).toBe(
      "openai",
    );
  },
);
