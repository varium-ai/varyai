import { readFileSync } from "node:fs";
import { expect, it, vi } from "vitest";
import { desktopFontPlatform } from "@/lib/platform";

it.each([
  ["Macintosh; Intel Mac OS X", '"PingFang SC"'],
  ["Windows NT 10.0; Win64; x64", '"Microsoft YaHei"'],
  ["X11; Linux x86_64", "system-ui"],
])("prefers the native UI font on %s", (userAgent, firstFont) => {
  const root = document.documentElement;
  const style = document.createElement("style");
  style.textContent = readFileSync("src/design-tokens.css", "utf8");
  document.head.appendChild(style);
  vi.stubGlobal("navigator", { userAgent, platform: "" });
  try {
    root.dataset.platform = desktopFontPlatform();
    const stack = getComputedStyle(root).getPropertyValue("--mp-font");
    expect(stack.split(",")[0].trim()).toBe(firstFont);
  } finally {
    delete root.dataset.platform;
    style.remove();
    vi.unstubAllGlobals();
  }
});
