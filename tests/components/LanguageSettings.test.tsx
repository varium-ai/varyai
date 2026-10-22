// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { LanguageSettings } from "@/components/settings/LanguageSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("LanguageSettings", () => {
  it("slides the selected background to the chosen language", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <LanguageSettings value="zh" onChange={onChange} />,
    );

    const indicator = screen.getByTestId("language-settings-indicator");
    expect(indicator).toHaveStyle({ transform: "translateX(0%)" });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.languageOptionEnglish",
      }),
    );
    expect(onChange).toHaveBeenCalledWith("en");

    rerender(<LanguageSettings value="en" onChange={onChange} />);
    expect(indicator).toHaveStyle({ transform: "translateX(200%)" });
    expect(
      screen.getByRole("button", {
        name: "settings.languageOptionEnglish",
      }),
    ).toHaveClass("hover:!text-white");
  });
});
