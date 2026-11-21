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

it.each([
  ["agents/agent-directory.css", ".mp-agent-row.agent-list-row"],
  ["providers/providers.css", ".vary-providers .pv-row"],
])("uses shared desktop list geometry in %s", (file, selector) => {
  const sheet = postcss.parse(readFileSync(`src/components/${file}`, "utf8"));
  const rule = sheet.nodes.find(
    (node) => node.type === "rule" && node.selector === selector,
  );
  expect(rule?.toString()).toContain("min-height: var(--workspace-row-height)");
  expect(rule?.toString()).toContain("padding: var(--workspace-row-padding)");
});

it("keeps Agent status text from pushing its action to the far edge", () => {
  const directory = postcss.parse(
    readFileSync("src/components/agents/agent-directory.css", "utf8"),
  );
  const declarations = new Map<string, string>();
  directory.walkRules(".agent-list-row .mp-agent-status-text", (rule) => {
    rule.walkDecls((declaration) => {
      declarations.set(declaration.prop, declaration.value);
    });
  });
  expect(declarations.get("flex")).toBe("0 1 auto");
  expect(declarations.get("white-space")).toBe("normal");
});

describe("workspace page heading consistency", () => {
  function declarationsFor(file: string, selector: string) {
    const sheet = postcss.parse(readFileSync(file, "utf8"));
    const declarations = new Map<string, string>();
    sheet.walkRules((rule) => {
      const selectors = rule.selector.split(",").map((part) => part.trim());
      if (selectors.includes(selector)) {
        rule.walkDecls((declaration) => {
          declarations.set(declaration.prop, declaration.value);
        });
      }
    });
    return declarations;
  }

  it("defines shared typography and responsive geometry tokens in workspace.css", () => {
    const topLevelInline = css.nodes
      .filter(
        (node) => node.type === "rule" && node.selector === ".vary-workspace",
      )
      .flatMap((node) => (node.type === "rule" ? node.nodes : []))
      .find(
        (decl) =>
          decl.type === "decl" && decl.prop === "--workspace-page-inline",
      );
    expect(value(".vary-workspace", "--workspace-page-max-width")).toBe(
      "1232px",
    );
    expect(topLevelInline?.type === "decl" && topLevelInline.value).toBe(
      "24px",
    );
    expect(value(".vary-workspace", "--workspace-page-inline")).toBe("12px");

    const heading = declarationsFor(
      "src/components/layout/workspace.css",
      ".workspace-page-heading > h1",
    );
    expect(heading.get("font-family")).toBe("var(--mp-font)");
    expect(heading.get("font-size")).toBe("18px");
    expect(heading.get("font-weight")).toBe("600");
    expect(heading.get("line-height")).toBe("26px");
    expect(heading.get("min-height")).toBe("var(--compact-control-height)");
  });

  it.each([
    ["src/components/layout/workspace.css", ".vary-agents"],
    ["src/components/layout/workspace.css", ".vary-routing"],
    ["src/components/layout/workspace.css", ".vary-providers"],
    ["src/components/routing/routing.css", ".vary-routing"],
    ["src/components/statistics/statistics-page.css", ".stats-page"],
    ["src/components/security/security-center.css", ".security-center"],
    ["src/components/settings/settings-page.css", ".settings-page"],
  ])(
    "uses the same max-width and responsive page padding in %s (%s)",
    (file, selector) => {
      const declarations = declarationsFor(file, selector);
      expect(declarations.get("max-width")).toBe(
        "var(--workspace-page-max-width, 1232px)",
      );
      expect(declarations.get("padding")).toContain(
        "var(--workspace-page-top, 24px)",
      );
      expect(declarations.get("padding")).toContain(
        "var(--workspace-page-inline, 24px)",
      );
    },
  );

  it("does not narrow SettingsPage with max-w-6xl and uses WorkspacePageHeading in ProviderAddSheet", () => {
    const settingsTsx = readFileSync(
      "src/components/settings/SettingsPage.tsx",
      "utf8",
    );
    expect(settingsTsx).not.toContain("max-w-6xl");

    const addSheetTsx = readFileSync(
      "src/components/providers/ProviderAddSheet.tsx",
      "utf8",
    );
    expect(addSheetTsx).toContain("WorkspacePageHeading");

    const providersCss = readFileSync(
      "src/components/providers/providers.css",
      "utf8",
    );
    expect(providersCss).not.toContain(".pv-add-copy h1");
  });
});
