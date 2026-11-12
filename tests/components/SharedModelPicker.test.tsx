import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { SharedModelPicker } from "@/components/models/SharedModelPicker";

it("updates provider icons in model rows and the provider rail", () => {
  const first =
    "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg'%3E%3Ccircle fill='red' r='8'/%3E%3C/svg%3E";
  const second =
    "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg'%3E%3Ccircle fill='blue' r='8'/%3E%3C/svg%3E";
  const props = {
    variant: "agent" as const,
    selected: [],
    models: [
      { id: "gs/model", name: "Synthetic model", group: "gs", icon: first },
    ],
  };
  const { rerender } = render(<SharedModelPicker {...props} />);
  const row = () => screen.getByRole("radio", { name: "Synthetic model" });
  const rail = () =>
    within(screen.getByRole("navigation")).getByRole("button", { name: "gs" });
  expect(row().querySelector(".mp-am-logo img")).toHaveAttribute("src", first);
  expect(rail().querySelector("img")).toHaveAttribute("src", first);
  rerender(
    <SharedModelPicker
      {...props}
      models={[{ ...props.models[0], icon: second }]}
    />,
  );
  expect(row().querySelector(".mp-am-logo img")).toHaveAttribute("src", second);
  expect(rail().querySelector("img")).toHaveAttribute("src", second);
  fireEvent.error(row().querySelector(".mp-am-logo img")!);
  expect(row().querySelector(".mp-am-logo img")).toBeNull();
  expect(row().querySelector(".mp-am-logo svg")).toBeInTheDocument();
});

it("uses the selected built-in or letter provider icon in model rows", () => {
  const props = {
    variant: "agent" as const,
    selected: [],
    models: [
      {
        id: "gs/model",
        name: "Synthetic model",
        group: "gs",
        icon: "generic-api",
      },
    ],
  };
  const { rerender } = render(<SharedModelPicker {...props} />);
  const row = () => screen.getByRole("radio", { name: "Synthetic model" });
  expect(
    row().querySelector('.mp-am-logo path[fill="#3F70FF"]'),
  ).toBeInTheDocument();
  rerender(
    <SharedModelPicker
      {...props}
      models={[{ ...props.models[0], icon: "letter" }]}
    />,
  );
  expect(row().querySelector(".mp-am-logo")).toHaveTextContent(/^GS$/);
});

it("keeps an explicit native model logo ahead of the provider icon", () => {
  render(
    <SharedModelPicker
      variant="agent"
      models={[
        {
          id: "native",
          name: "Native model",
          logo: "openai",
          icon: "letter",
          group: "gs",
          source: "native",
        },
      ]}
      selected={[]}
    />,
  );
  const row = screen.getByRole("radio", { name: "Native model" });
  expect(row.querySelector(".mp-am-logo svg")).toBeInTheDocument();
  expect(row.querySelector(".mp-am-logo")).not.toHaveTextContent("GS");
});

it.each([
  { variant: "agent" as const },
  {},
  { multiple: true },
  { readOnly: true },
])(
  "distinguishes same-name provider copies like Magpie with picker options %j",
  (options) => {
    const pick = vi.fn();
    render(
      <SharedModelPicker
        {...options}
        models={[
          {
            id: "vary-provider/model-original",
            reference: "gs/vendor/glm-5.3-flash",
            name: "GLM-5.3-Flash",
            group: "gs",
          },
          {
            id: "vary-provider/model-copy",
            reference: "gs-2/vendor/glm-5.3-flash",
            name: "GLM-5.3-Flash",
            group: "gs",
          },
        ]}
        selected={[]}
        onPick={pick}
      />,
    );
    const original = screen.getByTitle("GLM-5.3-Flash · gs");
    const copy = screen.getByTitle("GLM-5.3-Flash · gs-2");
    expect(original.querySelector(".mp-am-tell")).toHaveTextContent(/^gs$/);
    expect(copy.querySelector(".mp-am-tell")).toHaveTextContent(/^gs-2$/);
    if (!options.readOnly) {
      fireEvent.click(copy);
      expect(pick).toHaveBeenCalledWith("vary-provider/model-copy");
    }
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "gs-2" },
    });
    expect(screen.queryByTitle("GLM-5.3-Flash · gs")).not.toBeInTheDocument();
    expect(screen.getByTitle("GLM-5.3-Flash · gs-2")).toBeVisible();
  },
);

it("only adds Magpie identity hints for equal labels within the same provider group", () => {
  const { container } = render(
    <SharedModelPicker
      variant="agent"
      models={[
        { id: "relay/a", name: "Shared", group: "One" },
        { id: "other/a", name: "Shared", group: "Two" },
        { id: "relay/b", name: "SHARED", group: "One" },
      ]}
      selected={[]}
    />,
  );
  expect(container.querySelector(".mp-am-tell")).toBeNull();
});

it("keeps encoded classifier selections while displaying public model references", () => {
  const pick = vi.fn();
  const original = JSON.stringify(["gs", "vendor/glm"]);
  const copy = JSON.stringify(["gs-2", "vendor/glm"]);
  render(
    <SharedModelPicker
      models={[
        { id: original, reference: "gs/vendor/glm", name: "GLM", group: "gs" },
        { id: copy, reference: "gs-2/vendor/glm", name: "GLM", group: "gs" },
      ]}
      selected={[]}
      onPick={pick}
    />,
  );
  fireEvent.click(screen.getByRole("radio", { name: "GLM · gs-2" }));
  expect(pick).toHaveBeenCalledExactlyOnceWith(copy);
  expect(screen.queryByText(copy)).not.toBeInTheDocument();
});

it.each([
  { variant: "agent" as const },
  {},
  { multiple: true },
  { readOnly: true },
])("shows readable model tooltips with picker options %j", (options) => {
  const pick = vi.fn();
  const directId = `vary-provider/model-${"a".repeat(64)}`;
  const { container } = render(
    <SharedModelPicker
      {...options}
      models={[
        { id: directId, name: "GPT Example · Vendor", group: "Vendor" },
        {
          id: "vary-auto/group-example",
          name: "Smart · routing group",
          group: "routing",
        },
        {
          id: "native",
          modelId: "vendor/wire-model",
          name: "Native model",
          source: "native",
        },
      ]}
      selected={[]}
      onPick={pick}
    />,
  );
  expect(screen.getByTitle("GPT Example")).toHaveTextContent("GPT Example");
  expect(screen.getByTitle("Smart")).toHaveTextContent("Smart");
  expect(screen.getByTitle("Native model")).toHaveTextContent("Native model");
  expect(container.querySelector('[title^="vary-"]')).toBeNull();
  expect(screen.queryByTitle("vendor/wire-model")).not.toBeInTheDocument();
  if (!options.readOnly) {
    fireEvent.click(screen.getByTitle("GPT Example"));
    expect(pick).toHaveBeenCalledExactlyOnceWith(directId);
  }
});

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
