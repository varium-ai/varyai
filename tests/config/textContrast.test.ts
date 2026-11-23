import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import postcss from "postcss";
import { describe, expect, it } from "vitest";

type RGB = [number, number, number];
const css = postcss.parse(readFileSync("src/design-tokens.css", "utf8"));

function declarations(selector: string): Record<string, string> {
  const values: Record<string, string> = {};
  css.walkRules(selector, (rule) => {
    rule.walkDecls((declaration) => {
      values[declaration.prop] = declaration.value;
    });
  });
  return values;
}

const light = declarations(":root");
const dark = { ...light, ...declarations(".dark") };
const application = declarations(':root[data-renderer="application"]');
const applicationLight = {
  ...light,
  ...application,
  ...declarations(':root[data-renderer="application"]:not(.dark)'),
};
const applicationDark = {
  ...dark,
  ...application,
  ...declarations(':root[data-renderer="application"].dark'),
};

function color(value: string, tokens: Record<string, string>): RGB {
  const alias = value.match(/^var\((--[\w-]+)\)$/);
  if (alias) return color(tokens[alias[1]], tokens);
  if (value.startsWith("#")) {
    return [1, 3, 5].map(
      (i) => Number.parseInt(value.slice(i, i + 2), 16) / 255,
    ) as RGB;
  }
  const [h, s, l] = value.split(/\s+/).map(Number.parseFloat);
  if (![h, s, l].every(Number.isFinite))
    throw new Error(`Unsupported color: ${value}`);
  const a = (s / 100) * Math.min(l / 100, 1 - l / 100);
  return [0, 8, 4].map((n) => {
    const k = (n + h / 30) % 12;
    return l / 100 - a * Math.max(-1, Math.min(k - 3, 9 - k, 1));
  }) as RGB;
}
function contrast(a: RGB, b: RGB) {
  const luminance = (rgb: RGB) =>
    rgb
      .map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
      .reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
  const values = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (values[0] + 0.05) / (values[1] + 0.05);
}

describe.each([
  ["light", light],
  ["dark", dark],
] as const)("%s WCAG AA text palette", (_theme, tokens) => {
  const rgb = (name: string) => color(tokens[name], tokens);
  for (const foreground of [
    "--foreground",
    "--secondary-foreground",
    "--muted-foreground",
    "--link-foreground",
    "--success",
    "--warning",
    "--destructive",
  ]) {
    for (const background of [
      "--background",
      "--card",
      "--muted",
      "--accent",
    ]) {
      it(`${foreground} on ${background} meets 4.5:1 without rounding`, () => {
        expect(
          contrast(rgb(foreground), rgb(background)),
        ).toBeGreaterThanOrEqual(4.5);
      });
    }
  }
  it.each([
    ["--success", "--success-surface"],
    ["--warning", "--warning-surface"],
    ["--destructive", "--danger-surface"],
    ["--primary-foreground", "--primary-fill"],
    ["--primary-foreground", "--primary-fill-hover"],
  ])("%s on %s meets 4.5:1", (foreground, background) => {
    expect(contrast(rgb(foreground), rgb(background))).toBeGreaterThanOrEqual(
      4.5,
    );
  });
  it("filled action at 90% opacity keeps its white text readable", () => {
    const composite = rgb("--primary-fill").map(
      (v, i) => v * 0.9 + rgb("--card")[i] * 0.1,
    ) as RGB;
    expect(
      contrast(rgb("--primary-foreground"), composite),
    ).toBeGreaterThanOrEqual(4.5);
  });
});

it("uses the WCAG sRGB luminance endpoints", () => {
  expect(contrast([0, 0, 0], [1, 1, 1])).toBe(21);
  expect(contrast([0.5, 0.5, 0.5], [0.5, 0.5, 0.5])).toBe(1);
});

it("keeps text and filled backgrounds separate from brand/chart primary in Tailwind", () => {
  const require = createRequire(import.meta.url);
  const theme = require("../../tailwind.config.cjs").theme.extend;
  expect(theme.textColor.primary.DEFAULT).toBe("hsl(var(--primary-text))");
  expect(theme.backgroundColor.primary.DEFAULT).toBe(
    "hsl(var(--primary-action))",
  );
  expect(theme.backgroundColor.primary.foreground).toBe(
    "hsl(var(--primary-foreground))",
  );
  expect(theme.colors.primary.DEFAULT).toBe("hsl(var(--primary))");
});

describe.each([
  ["light", applicationLight],
  ["dark", applicationDark],
] as const)("%s application actions", (_theme, tokens) => {
  const rgb = (name: string) => color(tokens[name], tokens);

  it.each([
    ["--primary-text", "--card"],
    ["--primary-text", "--muted"],
    ["--primary-text", "--accent"],
    ["--primary-foreground", "--primary-action"],
    ["--primary-foreground", "--primary-action-hover"],
    ["--primary-foreground", "--mcp-action"],
    ["--primary-foreground", "--mcp-action-hover"],
  ])("keeps %s readable on %s", (foreground, background) => {
    expect(contrast(rgb(foreground), rgb(background))).toBeGreaterThanOrEqual(
      4.5,
    );
  });

  it("keeps the audit button readable in its default and hover states", () => {
    const component = readFileSync(
      "src/components/security/SecurityCenterPage.tsx",
      "utf8",
    );
    const match = component.match(
      /bg-\[(#[a-f0-9]{6})\] text-white hover:bg-amber-800/i,
    );
    expect(match).not.toBeNull();

    const require = createRequire(import.meta.url);
    const resolveConfig = require("tailwindcss/resolveConfig");
    const theme = resolveConfig(require("../../tailwind.config.cjs")).theme;
    for (const background of [match![1], theme.colors.amber[800]]) {
      expect(
        contrast(rgb("--primary-foreground"), color(background, tokens)),
      ).toBeGreaterThanOrEqual(4.5);
    }
  });
});
