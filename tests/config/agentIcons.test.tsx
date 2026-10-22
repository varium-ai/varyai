import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { APP_ICON_MAP, APP_ICON_NAME } from "@/config/appConfig";
import { getIconUrl, hasIcon } from "@/icons/extracted";

const agentIcons = [
  ["workbuddy", "workbuddy"],
  ["qoder", "qoder"],
  ["doubaowork", "doubaowork"],
  ["trae", "trae"],
  ["kimi", "kimi-code"],
  ["qwen", "qwen-code"],
  ["zcode", "zcode"],
] as const;

describe("official agent icons", () => {
  it.each(agentIcons)(
    "renders %s with its client asset instead of initials",
    (app, icon) => {
      expect(APP_ICON_NAME[app]).toBe(icon);
      expect(hasIcon(icon)).toBe(true);
      expect(getIconUrl(icon)).not.toBe("");
      const { container } = render(<>{APP_ICON_MAP[app].icon}</>);
      expect(container.querySelector("img")).toHaveAttribute(
        "src",
        getIconUrl(icon),
      );
    },
  );

  it("keeps client marks separate from model provider marks", () => {
    expect(APP_ICON_NAME.doubaowork).not.toBe("doubao");
    expect(APP_ICON_NAME.kimi).not.toBe("kimi");
    expect(APP_ICON_NAME.qwen).not.toBe("qwen");
    expect(hasIcon("doubao")).toBe(true);
    expect(hasIcon("kimi")).toBe(true);
    expect(hasIcon("qwen")).toBe(true);
  });
});
