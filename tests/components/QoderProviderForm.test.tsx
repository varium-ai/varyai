import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
vi.mock("sonner", () => ({
  toast: { error: vi.fn(), info: vi.fn(), success: vi.fn() },
}));
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    takeoverStatus: { qoder: false },
    setTakeoverForApp: vi.fn(),
  }),
}));
it("saves Qoder native models and protocol while retaining client capabilities", async () => {
  const onSubmit = vi.fn();
  render(
    <ProviderForm
      appId="qoder"
      submitLabel="Save Qoder"
      onSubmit={onSubmit}
      onCancel={vi.fn()}
      initialData={{
        name: "Gateway",
        settingsConfig: {
          baseUrl: "https://api.example/v1",
          apiKey: "synthetic-key",
          protocol: "openai-responses",
          type: "openai-compatible",
          authType: "bearer",
          model: "model-a",
          models: [
            {
              model: "model-a",
              displayName: "A",
              contextWindow: 100000,
              capabilities: { vision: true, custom: true },
            },
            { model: "model-b", displayName: "B" },
          ],
          future: { keep: true },
        },
      }}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Save Qoder" }));
  await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
  const config = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
  expect(config).toMatchObject({
    baseUrl: "https://api.example/v1",
    apiKey: "synthetic-key",
    protocol: "openai-responses",
    model: "model-a",
    future: { keep: true },
  });
  expect(config.models.map((m: any) => m.model)).toEqual([
    "model-a",
    "model-b",
  ]);
  expect(config.models[0].capabilities).toMatchObject({
    vision: true,
    custom: true,
  });
  expect(config).not.toHaveProperty("url");
  expect(config).not.toHaveProperty("id");
});

it("opens a styled protocol menu and saves the selected API protocol", async () => {
  Element.prototype.scrollIntoView = vi.fn();
  const user = userEvent.setup();
  const onSubmit = vi.fn();
  render(
    <ProviderForm
      appId="qoder"
      submitLabel="Save Qoder"
      onSubmit={onSubmit}
      onCancel={vi.fn()}
      initialData={{
        name: "Gateway",
        settingsConfig: {
          baseUrl: "https://api.example/v1",
          apiKey: "synthetic-key",
          protocol: "openai",
          authType: "bearer",
          model: "model-a",
          models: [{ model: "model-a", displayName: "A" }],
        },
      }}
    />,
  );
  await user.click(
    screen.getByRole("combobox", { name: "qoder.form.protocol" }),
  );
  expect(await screen.findByRole("listbox")).toBeVisible();
  await user.click(screen.getByRole("option", { name: "Anthropic Messages" }));
  expect(
    screen.getByRole("combobox", { name: "qoder.form.protocol" }),
  ).toHaveTextContent("Anthropic Messages");
  await user.click(screen.getByRole("button", { name: "Save Qoder" }));
  await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
  expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toMatchObject({
    protocol: "anthropic",
    authType: "api-key",
  });
});
