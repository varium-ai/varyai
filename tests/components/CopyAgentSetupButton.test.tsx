import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { beforeEach, expect, it, vi } from "vitest";
import { CopyAgentSetupButton } from "@/components/CopyAgentSetupButton";
import { copyText } from "@/lib/clipboard";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn() }));

beforeEach(() => {
  vi.mocked(copyText).mockResolvedValue(undefined);
});

it.each([
  { language: "en", translations: en, copied: "Copied" },
  { language: "zh", translations: zh, copied: "已复制" },
  { language: "ja", translations: ja, copied: "コピーしました" },
  { language: "zh-TW", translations: zhTW, copied: "已複製" },
])(
  "uses real $language translations for copy feedback and the prompt",
  async ({ language, translations, copied }) => {
    const i18n = createInstance();
    await i18n.init({
      lng: language,
      fallbackLng: false,
      resources: { [language]: { translation: translations } },
    });
    const onCopied = vi.fn();
    const onCopyFailed = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <CopyAgentSetupButton
          labelKey="onboarding.copyForAgent"
          onCopied={onCopied}
          onCopyFailed={onCopyFailed}
        />
      </I18nextProvider>,
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: translations.onboarding.copyForAgent,
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: copied })).toBeVisible(),
    );
    expect(screen.queryByText("common.copied")).not.toBeInTheDocument();
    expect(copyText).toHaveBeenCalledWith(translations.onboarding.helpPrompt);
    expect(onCopied).toHaveBeenCalledOnce();
    expect(onCopyFailed).not.toHaveBeenCalled();
  },
);
