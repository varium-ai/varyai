import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { RoutingRulesEditor } from "@/components/routing/RoutingRulesEditor";
import type { IntentRule } from "@/lib/api/intentRouting";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, args?: { n?: number }) =>
      args?.n ? `${key} ${args.n}` : key,
  }),
}));

const rules: IntentRule[] = [
  {
    id: "one",
    name: "Preserved legacy name",
    description: "Writing tests",
    enabled: true,
    targetMemberId: "a",
  },
  {
    id: "two",
    name: "Second",
    description: "",
    enabled: true,
    targetMemberId: "b",
    conditions: { images: true },
  },
];
const members = [
  { id: "a", providerId: "vendor", modelId: "large" },
  { id: "b", providerId: "relay", modelId: "small" },
  { id: "c", providerId: "relay", modelId: "medium" },
];
function Fixture({
  disabled = false,
  initial = rules,
}: {
  disabled?: boolean;
  initial?: IntentRule[];
}) {
  const [value, setValue] = useState(initial);
  return (
    <>
      <RoutingRulesEditor
        rules={value}
        members={members}
        disabled={disabled}
        onChange={setValue}
      />
      <output data-testid="rules">{JSON.stringify(value)}</output>
    </>
  );
}
const saved = () =>
  JSON.parse(screen.getByTestId("rules").textContent ?? "[]") as IntentRule[];

describe("the reference implementation routing rule sentences", () => {
  it("starts a new rule at the last model and focuses its intent without asking for a name", async () => {
    render(<Fixture />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "intentRouting.addRule" }),
    );
    const next = saved()[2];
    expect(next.targetMemberId).toBe("c");
    expect(next.name).toBe("intentRouting.ruleNumber 3");
    await waitFor(() =>
      expect(
        screen.getAllByLabelText("intentRouting.description")[1],
      ).toHaveFocus(),
    );
  });
  it("draws only set conditions, targets and actions, without a name or enable form", () => {
    render(<Fixture />);
    expect(screen.getByLabelText("intentRouting.description")).toHaveValue(
      "Writing tests",
    );
    expect(
      screen.queryByLabelText("intentRouting.ruleName"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("checkbox", { name: "intentRouting.ruleEnabled" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getAllByRole("button", { name: "routingPage.ruleActions 1" }),
    ).toHaveLength(1);
    expect(saved()[0].name).toBe("Preserved legacy name");
  });
  it("adds a condition from +, focuses it and keeps the chip while its value is empty", async () => {
    render(<Fixture />);
    const user = userEvent.setup();
    await user.click(
      screen.getAllByRole("button", { name: "intentRouting.addCondition" })[0],
    );
    await user.click(
      screen.getByRole("menuitem", { name: "intentRouting.conditionTokens" }),
    );
    const input = screen.getByLabelText("intentRouting.conditionTokens");
    await waitFor(() => expect(input).toHaveFocus());
    fireEvent.change(input, { target: { value: "4000" } });
    expect(saved()[0].conditions?.tokens).toBe(4000);
    fireEvent.change(input, { target: { value: "" } });
    expect(
      screen.getByLabelText("intentRouting.conditionTokens"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", {
        name: "intentRouting.removeCondition intentRouting.conditionTokens",
      }),
    );
    expect(
      screen.queryByLabelText("intentRouting.conditionTokens"),
    ).not.toBeInTheDocument();
  });
  it("reorders by keyboard and menu without recreating rule IDs or names", async () => {
    render(<Fixture />);
    const user = userEvent.setup();
    const handle = screen.getByRole("button", {
      name: "routingPage.moveRule 2",
    });
    handle.focus();
    fireEvent.keyDown(handle, { key: "ArrowUp", altKey: true });
    expect(saved().map((rule) => rule.id)).toEqual(["two", "one"]);
    expect(handle).toHaveFocus();
    await user.click(
      screen.getByRole("button", { name: "routingPage.ruleActions 1" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "intentRouting.moveDown" }),
    );
    expect(saved().map((rule) => rule.id)).toEqual(["one", "two"]);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "routingPage.ruleActions 2" }),
      ).toHaveFocus(),
    );
  });
  it("keeps fallback targets in selection order and removes the first without duplicates", async () => {
    render(<Fixture />);
    const user = userEvent.setup();
    const row = screen.getByLabelText("intentRouting.ruleNumber 1");
    await user.click(
      within(row).getByRole("button", { name: "routingPage.addRuleModel" }),
    );
    await user.click(screen.getByRole("menuitem", { name: "medium" }));
    await user.click(
      within(row).getByRole("button", { name: "routingPage.addRuleModel" }),
    );
    await user.click(screen.getByRole("menuitem", { name: "small" }));
    expect(saved()[0].fallbackMemberIds).toEqual(["c", "b"]);
    await user.click(
      within(row).getByRole("button", {
        name: "routingPage.removeRuleModel large",
      }),
    );
    expect(saved()[0]).toMatchObject({
      targetMemberId: "c",
      fallbackMemberIds: ["b"],
    });
  });
  it("pauses from the action menu, clears request-only fields and can send again", async () => {
    render(<Fixture />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "routingPage.ruleActions 1" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "routingPage.pauseRule" }),
    );
    expect(saved()[0]).toMatchObject({
      pause: true,
      description: "",
      fallbackMemberIds: [],
      useEffort: "",
    });
    expect(
      screen.queryByLabelText("intentRouting.description"),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getAllByRole("button", { name: "intentRouting.addCondition" })[0],
    );
    expect(
      screen.queryByRole("menuitem", { name: "intentRouting.conditionImages" }),
    ).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    await user.click(
      screen.getByRole("button", { name: "routingPage.ruleActions 1" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "routingPage.sendRule" }),
    );
    expect(saved()[0].pause).toBe(false);
  });
  it("disables all edits in manual mode without losing stored values", () => {
    render(<Fixture disabled />);
    expect(screen.getByLabelText("intentRouting.description")).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "routingPage.ruleActions 1" }),
    ).toBeDisabled();
    expect(saved()).toEqual(rules);
  });
  it("keeps a saved token chip while clearing it and keeps pause-only days without inventing hours", async () => {
    render(
      <Fixture
        initial={[
          { ...rules[0], description: "", conditions: { tokens: 4000 } },
        ]}
      />,
    );
    const user = userEvent.setup();
    fireEvent.change(screen.getByLabelText("intentRouting.conditionTokens"), {
      target: { value: "" },
    });
    expect(
      screen.getByLabelText("intentRouting.conditionTokens"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "routingPage.ruleActions 1" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "routingPage.pauseRule" }),
    );
    await user.click(
      screen.getByRole("button", { name: "intentRouting.addCondition" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "intentRouting.conditionTime" }),
    );
    await user.click(
      screen.getByRole("button", { name: "intentRouting.conditionTime" }),
    );
    await user.click(
      screen.getByRole("checkbox", { name: "intentRouting.weekdays.mon" }),
    );
    expect(saved()[0].conditions?.time).toMatchObject({
      from: "",
      to: "",
      days: ["mon"],
    });
  });
  it("closes the full action menu after choosing output effort", async () => {
    render(<Fixture />);
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "routingPage.ruleActions 1" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "intentRouting.ruleUseEffort" }),
    );
    await user.click(await screen.findByRole("menuitem", { name: "high" }));
    await waitFor(() =>
      expect(screen.queryByRole("menu")).not.toBeInTheDocument(),
    );
    expect(saved()[0].useEffort).toBe("high");
  });
});
