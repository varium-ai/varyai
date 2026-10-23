import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { expect, it, vi } from "vitest";
import { toast } from "sonner";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), info: vi.fn(), success: vi.fn() },
}));
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({ takeoverStatus: {}, setTakeoverForApp: vi.fn() }),
}));

const locales = [
  ["zh", zh, "请填写：", ["供应商名称", "模型 ID", "URL", "API Key"]],
  [
    "en",
    en,
    "Please fill in: ",
    ["Provider Name", "Model ID", "URL", "API Key"],
  ],
  [
    "ja",
    ja,
    "入力してください: ",
    ["プロバイダー名", "モデル ID", "URL", "API キー"],
  ],
  ["zh-TW", zhTW, "請填寫：", ["供應商名稱", "模型 ID", "URL", "API Key"]],
] as const;
const clients = ["qoder", "workbuddy"] as const;
const cases = clients.flatMap((appId) =>
  locales.flatMap(([language, translations, prefix, labels]) =>
    Array.from({ length: 15 }, (_, index) => ({
      appId,
      language,
      translations,
      prefix,
      labels,
      missing: index + 1,
    })),
  ),
);

it.each(cases)(
  "$appId lists exactly the missing fields for mask $missing in $language",
  async ({ appId, language, translations, prefix, labels, missing }) => {
    const i18n = createInstance();
    await i18n.init({
      lng: language,
      fallbackLng: false,
      resources: { [language]: { translation: translations } },
    });
    const name = missing & 1 ? "   " : "Gateway";
    const model = missing & 2 ? "   " : "model-a";
    const url = missing & 4 ? "   " : "https://api.example/v1";
    const apiKey = missing & 8 ? "   " : "synthetic-key";
    const onSubmit = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <ProviderForm
          appId={appId}
          submitLabel="Save provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name,
            settingsConfig:
              appId === "qoder"
                ? { model, baseUrl: url, apiKey }
                : { id: model, url, apiKey },
          }}
        />
      </I18nextProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save provider" }));
    const expected =
      prefix + labels.filter((_, i) => missing & (1 << i)).join(", ");
    await waitFor(
      () => expect(toast.error).toHaveBeenCalledExactlyOnceWith(expected),
      { timeout: 1000 },
    );
    expect(onSubmit).not.toHaveBeenCalled();
  },
);
