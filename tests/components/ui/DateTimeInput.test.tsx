import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DateTimeInput } from "@/components/ui/date-time-input";
describe("app date/time text editor", () => {
  it("keeps partial dates editable without committing invalid or rolled-over dates", () => {
    const changed = vi.fn();
    render(
      <DateTimeInput
        type="date"
        aria-label="Date"
        value="2026-11-21"
        onChange={changed}
      />,
    );
    const field = screen.getByLabelText("Date");
    expect(field).toHaveAttribute("type", "text");
    fireEvent.change(field, { target: { value: "2026-02-" } });
    expect(field).toHaveValue("2026-02-");
    expect(changed).not.toHaveBeenCalled();
    fireEvent.change(field, { target: { value: "2026-02-30" } });
    expect(changed).not.toHaveBeenCalled();
    fireEvent.blur(field);
    expect(field).toHaveValue("2026-11-21");
    fireEvent.change(field, { target: { value: "2028-02-29" } });
    expect(changed).toHaveBeenCalledTimes(1);
  });
  it("preserves minute precision, rejects out-of-range times and follows external calendar updates", () => {
    const changed = vi.fn();
    const { rerender } = render(
      <DateTimeInput
        type="time"
        aria-label="Time"
        value="00:00"
        onChange={changed}
      />,
    );
    const field = screen.getByLabelText("Time");
    fireEvent.change(field, { target: { value: "25:00" } });
    expect(changed).not.toHaveBeenCalled();
    fireEvent.change(field, { target: { value: "23:59" } });
    expect(changed).toHaveBeenCalledTimes(1);
    rerender(
      <DateTimeInput
        type="time"
        aria-label="Time"
        value="12:34"
        onChange={changed}
      />,
    );
    expect(field).toHaveValue("12:34");
  });
});
