import { readFileSync } from "node:fs";
import postcss from "postcss";
import { describe, expect, it } from "vitest";

const css = postcss.parse(
  readFileSync("src/components/layout/workspace.css", "utf8"),
);
function value(selector: string, property: string) {
  let result: string | undefined;
  css.walkRules(selector, (rule) => {
    rule.walkDecls(property, (declaration) => {
      result = declaration.value;
    });
  });
  return result;
}

describe("workspace titlebar spacing", () => {
  it("uses compact brand spacing below the Windows native titlebar", () => {
    expect(value(".vary-workspace", "--workspace-brand-top")).toBe("24px");
    expect(value(".vary-workspace", "--workspace-page-top")).toBe("24px");
  });

  it("preserves the macOS drag strip and existing content offsets", () => {
    expect(value(".workspace-drag-region", "height")).toBe("24px");
    expect(value(".workspace-drag-region + div", "--workspace-brand-top")).toBe(
      "24px",
    );
    expect(value(".workspace-drag-region + div", "--workspace-page-top")).toBe(
      "0px",
    );
  });
});
