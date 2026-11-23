import { describe, expect, it } from "vitest";
import { syncLanguage } from "@/hooks/useLanguageSync";

type FakeI18n = {
  language: string;
  changeLanguage: (language: string) => Promise<void>;
};

describe("shared tray and workspace language sync", () => {
  it("applies only supported settings languages", async () => {
    const calls: string[] = [];
    const i18n: FakeI18n = {
      language: "zh",
      changeLanguage: async (language) => {
        calls.push(language);
        i18n.language = language;
      },
    };

    for (const language of ["en", "zh", "zh-TW", "ja"]) {
      await syncLanguage(i18n, language);
    }
    expect(calls).toEqual(["en", "zh", "zh-TW", "ja"]);
  });

  it("does not repeat the current language or accept unsupported values", async () => {
    const calls: string[] = [];
    const i18n: FakeI18n = {
      language: "en",
      changeLanguage: async (language) => {
        calls.push(language);
      },
    };

    await syncLanguage(i18n, "en");
    await syncLanguage(i18n, null);
    await syncLanguage(i18n, "fr");
    expect(calls).toEqual([]);
  });
});
