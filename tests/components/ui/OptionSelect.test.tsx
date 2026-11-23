import { fireEvent, render, screen } from "@testing-library/react";
import { createRef, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { OptionSelect } from "@/components/ui/option-select";

function Fixture({ onSelect }: { onSelect: (value: string) => void }) {
  const [value, setValue] = useState("");
  return (
    <OptionSelect
      aria-label="Fixture selector"
      value={value}
      onValueChange={(next) => {
        setValue(next);
        onSelect(next);
      }}
    >
      <option value="">All items</option>
      <option value="alpha">Alpha</option>
      <option value="blocked" disabled>
        Unavailable
      </option>
    </OptionSelect>
  );
}
describe("app-owned option selector", () => {
  it("opens an app listbox and preserves values, empty options and disabled choices", () => {
    const changed = vi.fn();
    const { container } = render(<Fixture onSelect={changed} />);
    const trigger = screen.getByRole("combobox", { name: "Fixture selector" });
    expect(
      container.querySelector("select:not([aria-hidden=true])"),
    ).toBeNull();
    fireEvent.click(trigger);
    expect(screen.getByRole("listbox")).toBeVisible();
    expect(screen.getByRole("option", { name: "Unavailable" })).toHaveAttribute(
      "data-disabled",
    );
    fireEvent.click(screen.getByRole("option", { name: "Alpha" }));
    expect(changed).toHaveBeenLastCalledWith("alpha");
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("option", { name: "All items" }));
    expect(changed).toHaveBeenLastCalledWith("");
  });
  it("forwards focus to the trigger and disables opening", () => {
    const ref = createRef<HTMLButtonElement>();
    render(
      <OptionSelect ref={ref} disabled aria-label="Disabled selector" value="a">
        <option value="a">Alpha</option>
      </OptionSelect>,
    );
    expect(ref.current).toBe(screen.getByRole("combobox"));
    expect(ref.current).toBeDisabled();
  });
});
