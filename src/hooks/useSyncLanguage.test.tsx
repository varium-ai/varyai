import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSyncLanguage } from "./useSyncLanguage";

const changeLanguage = vi.fn().mockResolvedValue(undefined);
let currentLanguage = "zh";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    i18n: {
      get language() {
        return currentLanguage;
      },
      changeLanguage,
    },
  }),
}));

describe("useSyncLanguage", () => {
  beforeEach(() => {
    currentLanguage = "zh";
    changeLanguage.mockClear();
  });

  it("changes i18n language when settings language is supported and differs", () => {
    renderHook(({ language }) => useSyncLanguage(language), {
      initialProps: { language: "en" },
    });

    expect(changeLanguage).toHaveBeenCalledTimes(1);
    expect(changeLanguage).toHaveBeenCalledWith("en");
  });

  it("updates i18n language when settings language changes across renders", () => {
    const { rerender } = renderHook(
      ({ language }: { language?: string }) => useSyncLanguage(language),
      { initialProps: { language: "zh" } },
    );

    expect(changeLanguage).not.toHaveBeenCalled();

    rerender({ language: "ja" });
    expect(changeLanguage).toHaveBeenCalledWith("ja");
  });

  it("ignores unsupported or empty language values", () => {
    renderHook(
      ({ language }: { language?: string | null }) => useSyncLanguage(language),
      { initialProps: { language: "fr" as string | null } },
    );

    expect(changeLanguage).not.toHaveBeenCalled();
  });
});
