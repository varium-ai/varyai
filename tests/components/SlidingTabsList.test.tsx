// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

function rect(left: number, top: number, width: number, height: number) {
  return {
    x: left,
    y: top,
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    toJSON: () => ({}),
  } as DOMRect;
}

describe("TabsList", () => {
  afterEach(() => vi.restoreAllMocks());

  it("can keep a static selected tab without the shared sliding indicator", () => {
    render(
      <Tabs defaultValue="general">
        <TabsList aria-label="Settings categories" slidingIndicator={false}>
          <TabsTrigger
            value="general"
            className="data-[state=active]:!bg-blue-500"
          >
            General
          </TabsTrigger>
          <TabsTrigger value="advanced">Advanced</TabsTrigger>
        </TabsList>
        <TabsContent value="general">General settings</TabsContent>
        <TabsContent value="advanced">Advanced settings</TabsContent>
      </Tabs>,
    );

    expect(screen.queryByTestId("tabs-list-indicator")).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "General" })).toHaveClass(
      "data-[state=active]:!bg-blue-500",
    );
  });

  it("slides the shared indicator to the active Radix tab", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        if (this.getAttribute("role") === "tablist") {
          return rect(10, 20, 320, 44);
        }
        if (this.getAttribute("role") === "tab") {
          const tabs = Array.from(this.parentElement?.children ?? []).filter(
            (child) => child.getAttribute("role") === "tab",
          );
          const index = tabs.indexOf(this);
          return rect(10 + index * 100, 24, 96, 36);
        }
        return rect(0, 0, 0, 0);
      },
    );

    render(
      <Tabs defaultValue="events">
        <TabsList aria-label="Security sections">
          <TabsTrigger value="events">Events</TabsTrigger>
          <TabsTrigger value="rules">Rules</TabsTrigger>
        </TabsList>
        <TabsContent value="events">Event content</TabsContent>
        <TabsContent value="rules">Rule content</TabsContent>
      </Tabs>,
    );

    const indicator = screen.getByTestId("tabs-list-indicator");
    expect(indicator.style.transform).toBe("translate(0px, 4px)");

    fireEvent.mouseDown(screen.getByRole("tab", { name: "Rules" }), {
      button: 0,
      ctrlKey: false,
    });
    await waitFor(() => {
      expect(indicator.style.transform).toBe("translate(100px, 4px)");
    });
    expect(indicator).toHaveClass("transition-[transform,width]");
  });
});
