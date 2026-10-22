import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Switch } from "@/components/ui/switch";
import PromptToggle from "@/components/prompts/PromptToggle";

describe("switch visual language", () => {
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
