import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { SharedModelPicker } from "@/components/models/SharedModelPicker";

it("uses the same provider panel for single selection without visibility controls", () => {
  const pick = vi.fn();
  const { container } = render(
    <SharedModelPicker
      models={[
        {
          id: "native",
          name: "Native model",
          group: "Claude Code",
          disabled: true,
        },
        { id: "route", name: "Smart · routing group", group: "routing" },
        { id: "a", name: "Alpha · Vendor", group: "Vendor" },
      ]}
      selected={["native"]}
      onPick={pick}
    />,
  );
  expect(
    container.querySelector(".mp-shared-picker .mp-am-body"),
  ).not.toBeNull();
  const rail = screen.getByRole("navigation", {
    name: "agentConnections.modelProviders",
  });
  fireEvent.click(within(rail).getByRole("button", { name: "Vendor" }));
  expect(screen.getByRole("radio", { name: "Alpha" })).toBeInTheDocument();
  expect(
    screen.queryByRole("radio", { name: "Smart" }),
  ).not.toBeInTheDocument();
  fireEvent.change(screen.getByRole("searchbox"), {
    target: { value: "Smart" },
  });
  fireEvent.click(screen.getByRole("radio", { name: "Smart" }));
  expect(pick).toHaveBeenCalledExactlyOnceWith("route");
  expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "agentConnections.modelViewOrder" }),
  ).not.toBeInTheDocument();
});

it("filters grouped models and keeps the multi-picker open after each choice", () => {
  const onPick = vi.fn();
  const models = [
    { id: "a", name: "Alpha", group: "Vendor one", contextWindow: 1000000 },
    { id: "b", name: "Beta", group: "Vendor two" },
  ];
  const { rerender } = render(
    <SharedModelPicker
      models={models}
      selected={[]}
      onPick={onPick}
      multiple
    />,
  );
  fireEvent.click(screen.getByRole("checkbox", { name: /Alpha/ }));
  expect(onPick).toHaveBeenCalledWith("a");
  rerender(
    <SharedModelPicker
      models={models}
      selected={["a"]}
      onPick={onPick}
      multiple
    />,
  );
  expect(screen.getByRole("checkbox", { name: /Alpha/ })).toBeChecked();
  expect(screen.getByText("Vendor two")).toBeInTheDocument();
  fireEvent.change(screen.getByRole("searchbox"), {
    target: { value: "Vendor two" },
  });
  expect(
    screen.queryByRole("checkbox", { name: /Alpha/ }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox", { name: /Beta/ }));
  expect(onPick).toHaveBeenLastCalledWith("b");
});

it("isolates an unnamed provider instead of confusing it with All providers", () => {
  render(
    <SharedModelPicker
      models={[
        { id: "a", name: "Alpha" },
        { id: "b", name: "Beta", group: "Vendor one" },
        { id: "c", name: "Gamma", group: "Vendor two" },
      ]}
      selected={[]}
      onPick={vi.fn()}
    />,
  );
  fireEvent.click(
    within(screen.getByRole("navigation")).getByRole("button", {
      name: "modelProviders.title",
    }),
  );
  expect(screen.getByRole("radio", { name: "Alpha" })).toBeInTheDocument();
  expect(screen.queryByRole("radio", { name: "Beta" })).not.toBeInTheDocument();
  fireEvent.click(
    within(screen.getByRole("navigation")).getByRole("button", {
      name: "agentConnections.allProviders",
    }),
  );
  expect(screen.getByRole("radio", { name: "Beta" })).toBeInTheDocument();
});

it("keeps a long provider containing the current single choice expanded and disables picks when busy", () => {
  const pick = vi.fn();
  const { rerender } = render(
    <SharedModelPicker
      models={[
        ...Array.from({ length: 9 }, (_, index) => ({
          id: String(index),
          name: `Model ${index}`,
          group: "Vendor",
        })),
        { id: "other", name: "Other", group: "Other vendor" },
      ]}
      selected={["8"]}
      onPick={pick}
    />,
  );
  expect(screen.getByRole("radio", { name: "Model 8" })).toBeChecked();
  rerender(
    <SharedModelPicker
      models={[{ id: "a", name: "Alpha", group: "Vendor" }]}
      selected={[]}
      onPick={pick}
      disabled
    />,
  );
  fireEvent.keyDown(screen.getByRole("searchbox"), { key: "ArrowDown" });
  fireEvent.click(screen.getByRole("radio", { name: "Alpha" }));
  expect(pick).not.toHaveBeenCalled();
});

it("searches the exact model ID without adding reasoning or image badges", () => {
  render(
    <SharedModelPicker
      models={[
        {
          id: "alias-a",
          modelId: "exact-vendor-model",
          name: "My model",
          group: "Vendor",
          reasoningLevels: ["low", "medium", "high"],
          images: true,
        },
        { id: "alias-b", name: "Unknown model", group: "Vendor" },
      ]}
      selected={[]}
      onPick={vi.fn()}
    />,
  );
  expect(screen.queryByText("low–high")).not.toBeInTheDocument();
  expect(
    screen.queryByRole("img", { name: "modelProviders.acceptsImages" }),
  ).not.toBeInTheDocument();
  fireEvent.change(screen.getByRole("searchbox"), {
    target: { value: "exact-vendor-model" },
  });
  expect(screen.getByRole("radio", { name: /My model/ })).toBeInTheDocument();
  expect(
    screen.queryByRole("radio", { name: /Unknown model/ }),
  ).not.toBeInTheDocument();
});

it("uses a search-first Agent menu with icon navigation and explicit routing sources", () => {
  const pick = vi.fn();
  const disconnect = vi.fn();
  const { container } = render(
    <SharedModelPicker
      variant="agent"
      description="Configuration recovery information"
      models={[
        {
          id: "native",
          name: "Native model",
          source: "native",
          disabled: true,
        },
        { id: "route", name: "Smart · routing group", group: "routing" },
        { id: "a", name: "Alpha", group: "Vendor" },
      ]}
      selected={["native"]}
      onPick={pick}
      onDisconnect={disconnect}
    />,
  );
  const panel = container.querySelector(".mp-agent-model-picker")!;
  expect(panel.firstElementChild).toContainElement(
    screen.getByRole("searchbox"),
  );
  expect(panel.lastElementChild).toHaveTextContent(
    "Configuration recovery information",
  );
  const rail = screen.getByRole("navigation");
  expect(within(rail).getByRole("button", { name: "Vendor" })).toHaveAttribute(
    "title",
    "Vendor",
  );
  expect(within(rail).queryByText("Vendor")).not.toBeInTheDocument();
  expect(
    within(screen.getByRole("radio", { name: "Smart" })).getByText(
      "agentConnections.viaVary",
    ),
  ).toBeInTheDocument();
  expect(
    within(screen.getByRole("radio", { name: "Native model" })).queryByText(
      "agentConnections.viaVary",
    ),
  ).not.toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", { name: "agentConnections.restore" }),
  );
  expect(disconnect).toHaveBeenCalledOnce();
  expect(pick).not.toHaveBeenCalled();
  fireEvent.click(within(rail).getByRole("button", { name: "Vendor" }));
  expect(screen.getByRole("radio", { name: "Alpha" })).toBeInTheDocument();
  expect(
    screen.queryByRole("radio", { name: "Smart" }),
  ).not.toBeInTheDocument();
  fireEvent.change(screen.getByRole("searchbox"), {
    target: { value: "Smart" },
  });
  fireEvent.keyDown(screen.getByRole("searchbox"), { key: "ArrowDown" });
  expect(screen.getByRole("radio", { name: "Smart" })).toHaveFocus();
  fireEvent.keyDown(screen.getByRole("radio", { name: "Smart" }), {
    key: "Enter",
  });
  expect(pick).toHaveBeenCalledExactlyOnceWith("route");
});

it("moves through available choices with the keyboard and keeps the multi-picker open", () => {
  const pick = vi.fn();
  render(
    <SharedModelPicker
      models={[
        { id: "a", name: "Alpha", group: "Vendor" },
        { id: "off", name: "Disabled", group: "Vendor", disabled: true },
        { id: "b", name: "Beta", group: "Vendor" },
      ]}
      selected={[]}
      onPick={pick}
      multiple
    />,
  );
  fireEvent.keyDown(screen.getByRole("searchbox"), { key: "ArrowDown" });
  expect(screen.getByRole("checkbox", { name: "Alpha" })).toHaveFocus();
  fireEvent.keyDown(screen.getByRole("checkbox", { name: "Alpha" }), {
    key: "ArrowDown",
  });
  expect(screen.getByRole("checkbox", { name: "Beta" })).toHaveFocus();
  fireEvent.keyDown(screen.getByRole("checkbox", { name: "Beta" }), {
    key: "Enter",
  });
  expect(pick).toHaveBeenCalledWith("b");
  expect(screen.getByRole("checkbox", { name: "Alpha" })).toBeInTheDocument();
});

it("does not apply an already selected starting model again", () => {
  const pick = vi.fn();
  render(
    <SharedModelPicker
      models={[{ id: "a", name: "Alpha", group: "Vendor" }]}
      selected={["a"]}
      onPick={pick}
    />,
  );
  fireEvent.click(screen.getByRole("radio", { name: "Alpha" }));
  expect(pick).not.toHaveBeenCalled();
});
