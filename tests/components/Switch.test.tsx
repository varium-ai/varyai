import { render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import postcss from "postcss";
import { describe, expect, it } from "vitest";
import { Switch } from "@/components/ui/switch";
import PromptToggle from "@/components/prompts/PromptToggle";

describe("switch visual language", () => {
  it("keeps dialog button rounding off the provider switch track", () => {
    render(
      <div role="dialog" className="mp-dialog pv-provider-dialog">
        <section className="pv-editor">
          <header className="pv-editor-head">
            <button type="button">Provider website</button>
            <Switch checked className="mp-switch" aria-label="Enabled" />
          </header>
        </section>
      </div>,
    );

    const switchElement = screen.getByRole("switch");
    const website = screen.getByRole("button", { name: "Provider website" });
    const rounding = postcss
      .parse(readFileSync("src/components/layout/workspace.css", "utf8"))
      .nodes.find(
        (node) =>
          node.type === "rule" &&
          website.matches(node.selector) &&
          node.nodes.some(
            (child) => child.type === "decl" && child.prop === "border-radius",
          ),
      );

    expect(rounding?.type).toBe("rule");
    if (rounding?.type !== "rule")
      throw new Error("Missing dialog button rule");
    expect(switchElement.matches(rounding.selector)).toBe(false);
  });

  it("uses the primary track color for the shared switch", () => {
    render(<Switch checked onCheckedChange={() => undefined} />);

    const switchElement = screen.getByRole("switch");
    expect(switchElement).toHaveClass("data-[state=checked]:bg-primary");
    expect(switchElement).toHaveClass("data-[state=unchecked]:bg-gray-200");
    expect(switchElement.firstElementChild).toHaveClass(
      "h-5",
      "w-5",
      "bg-white",
    );
  });

  it("uses the same primary switch treatment for prompt toggles", () => {
    render(<PromptToggle enabled onChange={() => undefined} />);

    const switchElement = screen.getByRole("switch");
    expect(switchElement).toHaveClass("bg-primary");
    expect(switchElement.firstElementChild).toHaveClass(
      "h-5",
      "w-5",
      "translate-x-5",
      "shadow-lg",
    );
  });
});
