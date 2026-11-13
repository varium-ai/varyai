import { render, screen, within } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { expect, it } from "vitest";
import { SharedModelPicker } from "@/components/models/SharedModelPicker";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

it.each([
  ["en", en, "via Vary AI"],
  ["zh", zh, "经 Vary AI"],
  ["ja", ja, "Vary AI 経由"],
  ["zh-TW", zhTW, "經 Vary AI"],
] as const)(
  "brands managed model sources in %s",
  async (language, translation, label) => {
    const i18n = createInstance();
    await i18n.use(initReactI18next).init({
      lng: language,
      resources: { [language]: { translation } },
      interpolation: { escapeValue: false },
    });
    render(
      <I18nextProvider i18n={i18n}>
        <SharedModelPicker
          variant="agent"
          selected={[]}
          models={[
            { id: "route", name: "Synthetic route", group: "routing" },
            { id: "managed", name: "Synthetic model", group: "Vendor" },
            { id: "native", name: "Native model", source: "native" },
          ]}
        />
      </I18nextProvider>,
    );
    for (const name of ["Synthetic route", "Synthetic model"]) {
      const row = screen.getByRole("radio", { name });
      expect(within(row).getByText(label)).toBeVisible();
      expect(row).not.toHaveTextContent(/magpie/i);
    }
    const native = screen.getByRole("radio", { name: "Native model" });
    expect(
      within(native).getByText(
        translation.agentConnections.nativeConfiguration,
      ),
    ).toBeVisible();
    expect(within(native).queryByText(label)).toBeNull();
  },
);
