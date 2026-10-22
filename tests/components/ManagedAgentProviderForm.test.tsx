import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { ManagedAgentProviderForm } from "@/components/providers/forms/ManagedAgentProviderForm";

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
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
