import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { RoutingExecutionSettings } from "@/components/routing/RoutingExecutionSettings";
import { blankRoutingGroup } from "@/lib/routingGroupTransfer";
import type { RoutingGroup } from "@/lib/api/intentRouting";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
function Fixture({
  first = 0,
  patience = -1,
}: {
  first?: number;
  patience?: number;
}) {
  const [value, setValue] = useState<RoutingGroup>({
    ...blankRoutingGroup(),
    firstToken: first,
    patience,
  });
  return (
    <>
      <RoutingExecutionSettings
        group={value}
        disabled={false}
        onChange={setValue}
      />
      <output data-testid="value">{JSON.stringify(value)}</output>
    </>
  );
}
const saved = () => JSON.parse(screen.getByTestId("value").textContent ?? "{}");
describe("the reference implementation execution settings", () => {
  it("offers reference context choices and accepts k/m token sizes with focused custom input", () => {
    render(<Fixture />);
    fireEvent.click(
      screen.getByRole("radio", { name: "intentRouting.contextSmallest" }),
    );
    expect(saved().contextWindow).toBe(-1);
    fireEvent.click(
      screen.getByRole("radio", { name: "intentRouting.contextCustom" }),
    );
    const input = screen.getByRole("textbox", {
      name: "intentRouting.contextTokens",
    });
    expect(input).toHaveFocus();
    fireEvent.change(input, { target: { value: "1.5m" } });
    expect(saved().contextWindow).toBe(1500000);
    fireEvent.change(input, { target: { value: "272k" } });
    expect(saved().contextWindow).toBe(272000);
    fireEvent.change(input, { target: { value: "not a capacity" } });
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(saved().contextWindow).toBeNull();
    fireEvent.click(
      screen.getByRole("radio", { name: "intentRouting.contextLargest" }),
    );
    expect(saved().contextWindow).toBeUndefined();
  });
  it("uses first-content presets and minutes for custom values, saved as seconds", () => {
    render(<Fixture />);
    fireEvent.click(screen.getByRole("radio", { name: "30 s" }));
    expect(saved().firstToken).toBe(30);
    fireEvent.click(
      screen.getByRole("radio", { name: "routingPage.customTimeout" }),
    );
    expect(screen.getByLabelText("routingPage.timeoutMinutes")).toHaveValue(
      "0.5",
    );
    expect(screen.getByLabelText("routingPage.timeoutMinutes")).toHaveFocus();
    fireEvent.change(screen.getByLabelText("routingPage.timeoutMinutes"), {
      target: { value: "1,5" },
    });
    expect(saved().firstToken).toBe(90);
    fireEvent.click(
      screen.getByRole("radio", { name: "routingPage.waitForStart" }),
    );
    expect(saved().firstToken).toBe(0);
    expect(
      screen.queryByLabelText("routingPage.timeoutMinutes"),
    ).not.toBeInTheDocument();
  });
  it("retains inherited custom timeouts and patience until an explicit choice", () => {
    render(<Fixture first={5} patience={20} />);
    expect(saved()).toMatchObject({ firstToken: 5, patience: 20 });
    fireEvent.click(
      screen.getByRole("radio", { name: "routingPage.patienceTwoMinutes" }),
    );
    expect(saved().patience).toBe(120);
    fireEvent.click(
      screen.getByRole("radio", { name: "routingPage.patienceOneMinute" }),
    );
    expect(saved().patience).toBe(0);
    fireEvent.click(
      screen.getByRole("radio", { name: "routingPage.patienceOff" }),
    );
    expect(saved().patience).toBe(-1);
  });
  it("marks invalid minutes instead of silently keeping a valid previous value", () => {
    render(<Fixture first={5} />);
    fireEvent.change(screen.getByLabelText("routingPage.timeoutMinutes"), {
      target: { value: "oops" },
    });
    expect(screen.getByLabelText("routingPage.timeoutMinutes")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
    expect(saved().firstToken).toBeNull();
  });
});
