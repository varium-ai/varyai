// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SettingsFormState } from "@/hooks/useSettings";
import { AppVisibilitySettings } from "./AppVisibilitySettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => null,
}));

it("does not offer GLM visibility even when an old setting enables it", () => {
  render(
    <AppVisibilitySettings
      settings={{ visibleApps: { glm: true } } as SettingsFormState}
      onChange={vi.fn()}
    />,
  );
  expect(
    screen.queryByRole("button", { name: "apps.glm" }),
  ).not.toBeInTheDocument();
});

describe("AppVisibilitySettings profile switcher", () => {
  it.each([undefined, true, false])(
    "preserves the visible-by-default setting and saves changes from %s",
    (showProfileSwitcher) => {
      const onChange = vi.fn();
      render(
        <AppVisibilitySettings
          settings={{ showProfileSwitcher } as SettingsFormState}
          onChange={onChange}
        />,
      );
      const toggle = screen.getByRole("switch", {
        name: "settings.appVisibility.showProfileSwitcher",
      });
      expect(toggle).toHaveAttribute(
        "aria-checked",
        String(showProfileSwitcher ?? true),
      );
      fireEvent.click(toggle);
      expect(onChange).toHaveBeenCalledWith({
        showProfileSwitcher: !(showProfileSwitcher ?? true),
      });
    },
  );
});
