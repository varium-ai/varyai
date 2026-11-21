import { readFileSync } from "node:fs";
import { render } from "@testing-library/react";
import postcss from "postcss";
import { expect, it } from "vitest";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SearchInput } from "@/components/ui/search-input";

it("centers the decorative search icon in the input row regardless of height", () => {
  const { getByRole } = render(<SearchInput aria-label="Find tools" />);
  const input = getByRole("searchbox", { name: "Find tools" });
  expect(input.parentElement).toHaveClass("flex", "items-center");
  expect(input.previousElementSibling).toHaveClass(
    "absolute",
    "pointer-events-none",
  );
  expect(input.previousElementSibling).toHaveAttribute("aria-hidden", "true");
});

it("uses the shared compact control height for standard buttons, inputs, and routing filters", () => {
  const { getByRole } = render(
    <>
      <Button>Add provider</Button>
      <Button size="sm">Create group</Button>
      <Button size="icon" aria-label="Back" />
      <Input aria-label="Name" />
    </>,
  );
  const utility = "h-[var(--compact-control-height)]";
  expect(getByRole("button", { name: "Add provider" })).toHaveClass(utility);
  expect(getByRole("button", { name: "Create group" })).toHaveClass(utility);
  expect(getByRole("button", { name: "Back" })).toHaveClass(utility);
  expect(getByRole("textbox")).toHaveClass(utility);

  const tokens = readFileSync("src/design-tokens.css", "utf8");
  expect(tokens).toContain("--compact-control-height: 2rem;");
  const dialogCss = postcss.parse(
    readFileSync("src/components/ui/dialog.css", "utf8"),
  );
  const footerHeights: string[] = [];
  dialogCss.walkRules(".vary-dialog-footer > button", (rule) => {
    rule.walkDecls("min-height", (decl) => {
      footerHeights.push(decl.value);
    });
  });
  expect(footerHeights).toEqual(["var(--compact-control-height)"]);
  const rules = postcss.parse(
    readFileSync("src/components/routing/routing.css", "utf8"),
  );
  for (const selector of [
    ".vary-routing .rt-group-search input",
    ".vary-routing .rt-group-tools .rt-group-status-filter select",
  ]) {
    const heights: string[] = [];
    rules.walkRules(selector, (rule) => {
      rule.walkDecls("height", (decl) => {
        heights.push(decl.value);
      });
    });
    expect(heights).toEqual(["var(--compact-control-height)"]);
  }
});
