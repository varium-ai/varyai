import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import { expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { toast } from "sonner";
vi.mock("sonner", () => ({
  toast: { error: vi.fn(), info: vi.fn(), success: vi.fn() },
}));
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    takeoverStatus: { qoder: false },
    setTakeoverForApp: vi.fn(),
  }),
}));
const nameValidationLocales = [
  ["zh", zh, "请填写：供应商名称"],
  ["en", en, "Please fill in: Provider Name"],
  ["ja", ja, "入力してください: プロバイダー名"],
  ["zh-TW", zhTW, "請填寫：供應商名稱"],
] as const;

it.each(
  nameValidationLocales.flatMap(([language, translations, message]) =>
    ["", "   "].map((name) => ({ language, translations, message, name })),
  ),
)(
  "renders a localized missing-name error in $language for '$name'",
  async ({ language, translations, message, name }) => {
    const i18n = createInstance();
    await i18n.init({
      lng: language,
      fallbackLng: false,
      resources: { [language]: { translation: translations } },
    });
    const onSubmit = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <ProviderForm
          appId="qoder"
          submitLabel="Save Qoder"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name,
            settingsConfig: {
              baseUrl: "https://api.example/v1",
              apiKey: "synthetic-key",
              model: "model-a",
            },
          }}
        />
      </I18nextProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save Qoder" }));
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith(message));
    expect(onSubmit).not.toHaveBeenCalled();
  },
);
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
    screen.getByRole("combobox", { name: "providerForm.apiFormat" }),
  );
  expect(await screen.findByRole("listbox")).toBeVisible();
  await user.click(
    screen.getByRole("option", { name: "providerForm.apiFormatAnthropic" }),
  );
  expect(
    screen.getByRole("combobox", { name: "providerForm.apiFormat" }),
  ).toHaveTextContent("providerForm.apiFormatAnthropic");
  await user.click(screen.getByRole("button", { name: "Save Qoder" }));
  await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
  expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toMatchObject({
    protocol: "anthropic",
    authType: "api-key",
  });
});
