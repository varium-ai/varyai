import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { IconPicker } from "@/components/IconPicker";
import { ProviderIconField } from "@/components/providers/ProviderIconField";
import { ProviderIcon } from "@/components/ProviderIcon";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";

afterEach(() => vi.restoreAllMocks());

it("removes icon acquisition and restores a persisted random built-in icon", () => {
  const changed = vi.fn();
  const draft = {
    id: "synthetic",
    name: "Synthetic",
    icon: "generic-api",
    source: { kind: "api", baseUrl: "https://example.com/v1" },
  } as RoutingUpstream;
  render(
    <ProviderIconField
      draft={draft}
      disabled={false}
      onChange={changed}
      onBusyChange={vi.fn()}
    />,
  );
  expect(
    screen.queryByRole("button", { name: "providerEditor.getIcon" }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "providerEditor.imageUrl" }),
  ).not.toBeInTheDocument();
  vi.spyOn(Math, "random").mockReturnValue(0.999);
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.defaultIcon" }),
  );
  expect(changed).toHaveBeenCalledWith({ ...draft, icon: "generic-memory" });
});

it("offers the selected artwork and searches generic concepts in the form picker", () => {
  const changed = vi.fn();
  render(<IconPicker onValueChange={changed} />);
  expect(screen.getAllByRole("button")).toHaveLength(15);
  expect(screen.queryByTitle("OpenAI")).not.toBeInTheDocument();
  fireEvent.change(screen.getByRole("textbox"), {
    target: { value: "知识库" },
  });
  fireEvent.click(screen.getByRole("button"));
  expect(changed).toHaveBeenCalledWith("generic-knowledge");
});

it("uses the same curated icons in the upstream editor and persists the selected key", () => {
  const changed = vi.fn();
  const draft = {
    id: "synthetic",
    name: "Synthetic",
    schemaVersion: 1,
    source: { kind: "api", baseUrl: "https://example.com/v1" },
  } as RoutingUpstream;
  render(
    <ProviderIconField
      draft={draft}
      disabled={false}
      onChange={changed}
      onBusyChange={vi.fn()}
    />,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.builtinIcons" }),
  );
  const options = screen
    .getAllByRole("button")
    .filter((button) => button.hasAttribute("aria-pressed"));
  expect(options).toHaveLength(15);
  fireEvent.change(screen.getByRole("searchbox"), {
    target: { value: "记忆" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Memory" }));
  expect(changed).toHaveBeenCalledWith({ ...draft, icon: "generic-memory" });
});

it("renders a new colored vector and keeps existing saved brand keys readable", () => {
  const { container } = render(
    <ProviderIcon icon="generic-api" name="Synthetic" />,
  );
  expect(
    container.querySelector("svg path[fill='#3F70FF']"),
  ).toBeInTheDocument();
  const old = render(<ProviderIcon icon="openai" name="Synthetic legacy" />);
  expect(old.container.querySelector("svg")).toBeInTheDocument();
});
