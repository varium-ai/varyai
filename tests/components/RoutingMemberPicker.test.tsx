import { useState } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, it } from "vitest";
import { RoutingMemberPicker } from "@/components/routing/RoutingMemberPicker";
import type {
  RoutingMember,
  RoutingProviderOption,
} from "@/lib/api/intentRouting";
import { routingMemberKey } from "@/lib/routingMemberSelection";

const providers: RoutingProviderOption[] = [
  {
    id: "fixture",
    name: "Synthetic provider",
    taskEligible: true,
    classifierEligible: false,
    models: [
      { id: "current", name: "current" },
      { id: "second", name: "second" },
    ],
  },
];
function Harness() {
  const [members, setMembers] = useState<RoutingMember[]>([
    { id: "old", providerId: "fixture", modelId: "removed" },
  ]);
  return (
    <RoutingMemberPicker
      providers={providers}
      members={members}
      disabled={false}
      onSelect={(choices, selected) =>
        setMembers((current) =>
          selected
            ? [
                ...current,
                ...choices
                  .filter(
                    (choice) =>
                      !current.some(
                        (member) =>
                          routingMemberKey(member) === routingMemberKey(choice),
                      ),
                  )
                  .map((choice) => ({ ...choice, id: choice.modelId })),
              ]
            : current.filter(
                (member) =>
                  !choices.some(
                    (choice) =>
                      routingMemberKey(choice) === routingMemberKey(member),
                  ),
              ),
        )
      }
    />
  );
}
it("marks removed provider models unavailable and never adds them back via select-all", () => {
  render(<Harness />);
  const removed = screen.getByRole("checkbox", { name: "removed" });
  expect(
    within(removed.closest("label")!).getByText("intentRouting.memberNotReady"),
  ).toBeVisible();
  fireEvent.click(removed);
  expect(
    screen.queryByRole("checkbox", { name: "removed" }),
  ).not.toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.selectAll" }),
  );
  expect(screen.getByRole("checkbox", { name: "current" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "second" })).toBeChecked();
  expect(
    screen.queryByRole("checkbox", { name: "removed" }),
  ).not.toBeInTheDocument();
});
it("keeps the remaining model choices usable after selecting the first", () => {
  render(<Harness />);
  fireEvent.click(screen.getByRole("checkbox", { name: "current" }));
  expect(screen.getByRole("checkbox", { name: "second" })).toBeEnabled();
  fireEvent.click(screen.getByRole("checkbox", { name: "second" }));
  expect(screen.getByRole("checkbox", { name: "current" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "second" })).toBeChecked();
});
