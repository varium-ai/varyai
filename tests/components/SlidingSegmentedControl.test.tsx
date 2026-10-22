// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SlidingSegmentedControl } from "@/components/ui/sliding-segmented-control";

describe("SlidingSegmentedControl", () => {
  it("moves a shared indicator between equal-width tabs", () => {
    const onValueChange = vi.fn();
    const options = [
      { value: "events", label: "Events" },
      { value: "rules", label: "Rules" },
      { value: "risk", label: "Risk" },
    ];
    const { rerender } = render(
      <SlidingSegmentedControl
        value="events"
        onValueChange={onValueChange}
        options={options}
        role="tablist"
        itemRole="tab"
        ariaLabel="Security sections"
        indicatorVariant="surface"
        indicatorTestId="security-tabs-indicator"
      />,
    );

    const indicator = screen.getByTestId("security-tabs-indicator");
    expect(indicator).toHaveStyle({ transform: "translateX(0%)" });
    expect(screen.getByRole("tab", { name: "Events" })).toHaveAttribute(
      "aria-selected",
      "true",
    );

    fireEvent.click(screen.getByRole("tab", { name: "Rules" }));
    expect(onValueChange).toHaveBeenCalledWith("rules");

    rerender(
      <SlidingSegmentedControl
        value="rules"
        onValueChange={onValueChange}
        options={options}
        role="tablist"
        itemRole="tab"
        ariaLabel="Security sections"
        indicatorVariant="surface"
        indicatorTestId="security-tabs-indicator"
      />,
    );
    expect(indicator).toHaveStyle({ transform: "translateX(100%)" });
    expect(screen.getByRole("tab", { name: "Rules" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
});
