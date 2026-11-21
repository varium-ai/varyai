import { readFileSync } from "node:fs";
import postcss from "postcss";
import { describe, expect, it } from "vitest";

const tokens = postcss.parse(readFileSync("src/design-tokens.css", "utf8"));

function luminance(color: string) {
  let rgb: number[];
  if (color.startsWith("#")) {
    rgb = color
      .slice(1)
      .match(/../g)!
      .map((channel) => parseInt(channel, 16) / 255);
  } else {
    const [h, s, l] = color.split(/\s+/).map(parseFloat);
    const saturation = s / 100;
    const lightness = l / 100;
    const chroma = saturation * Math.min(lightness, 1 - lightness);
    rgb = [0, 8, 4].map((offset) => {
      const sector = (offset + h / 30) % 12;
      return (
        lightness - chroma * Math.max(-1, Math.min(sector - 3, 9 - sector, 1))
      );
    });
  }
  const linear = rgb.map((value) =>
    value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4,
  );
  return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
}

describe.each(["light", "dark"])("%s text contrast tokens", (theme) => {
  function token(name: string) {
    let color: string | undefined;
    const selectors =
      theme === "dark"
        ? [
            ":root",
            ".dark",
            ':root[data-renderer="application"]',
            ':root[data-renderer="application"].dark',
          ]
        : [
            ":root",
            ':root[data-renderer="application"]',
            ':root[data-renderer="application"]:not(.dark)',
          ];
    for (const selector of selectors) {
      tokens.walkRules(selector, (rule) => {
        rule.walkDecls(`--${name}`, (declaration) => {
          color = declaration.value;
        });
      });
    }
    if (!color) throw new Error(`Missing --${name}`);
    return luminance(color);
  }

  it.each([
    ["foreground", "background"],
    ["foreground", "muted"],
    ["foreground", "accent"],
    ["secondary-foreground", "card"],
    ["muted-foreground", "background"],
    ["muted-foreground", "muted"],
    ["primary-text", "card"],
    ["primary-text", "muted"],
    ["primary-text", "accent"],
    ["primary-foreground", "primary-action"],
    ["primary-foreground", "primary-action-hover"],
    ["primary-foreground", "mcp-action"],
    ["primary-foreground", "mcp-action-hover"],
    ["success", "success-surface"],
    ["warning", "warning-surface"],
    ["destructive", "danger-surface"],
  ])("keeps %s readable on %s", (foreground, background) => {
    const values = [token(foreground), token(background)];
    expect(
      (Math.max(...values) + 0.05) / (Math.min(...values) + 0.05),
    ).toBeGreaterThanOrEqual(4.5);
  });
});
