import { render, screen } from "@testing-library/react";
import i18n from "i18next";
import { beforeEach, expect, it } from "vitest";
import { UsageSourceLabel } from "@/components/usage/UsageSourceLabel";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";

beforeEach(() => {
  for (const [language, messages] of Object.entries({
    en,
    zh,
    "zh-TW": zhTW,
    ja,
  })) {
    i18n.addResourceBundle(
      language,
      "translation",
      { usage: messages.usage },
      true,
      true,
    );
  }
  void i18n.changeLanguage("en");
});

it.each(["en", "zh", "zh-TW", "ja"])(
  "shows the plain Codex source name in %s",
  (language) => {
    void i18n.changeLanguage(language);
    render(
      <UsageSourceLabel
        source={{
          providerId: "_codex_session",
          providerName: "Codex (Session)",
        }}
      />,
    );
    expect(screen.getByText("Codex")).toBeVisible();
    expect(screen.queryByText("Codex (Session)")).not.toBeInTheDocument();
    expect(
      screen.queryByText(/Session import|会话导入|工作階段匯入|セッション取込/),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Codex").parentElement).not.toHaveAttribute(
      "title",
    );
  },
);

it.each([
  ["_session", "Claude (Session)", "Claude"],
  ["_gemini_session", "Gemini (Session)", "Gemini"],
  ["_opencode_session", "OpenCode (Session)", "OpenCode"],
  ["_grok_session", "Grok Build (Session)", "Grok Build"],
  ["_pi_session", "Pi (Session)", "Pi"],
  ["varyai", "Pi (Session)", "Pi"],
])(
  "shows a plain app name for the known source %s",
  (providerId, providerName, app) => {
    render(<UsageSourceLabel source={{ providerId, providerName }} />);
    expect(screen.getByText(app)).toBeVisible();
    expect(screen.queryByText("Provider unattributed")).not.toBeInTheDocument();
  },
);

it.each([
  ["saved", "Codex (Session)"],
  ["saved", "Pi (Session)"],
  ["_codex_session", "Synthetic supplier"],
  ["varyai", "Vary AI"],
])(
  "preserves an actual or historical provider name: %s / %s",
  (providerId, providerName) => {
    render(<UsageSourceLabel source={{ providerId, providerName }} />);
    expect(screen.getByText(providerName)).toBeVisible();
    expect(screen.queryByText("Provider unattributed")).not.toBeInTheDocument();
  },
);
