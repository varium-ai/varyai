import { StrictMode } from "react";
import { act, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useRoutingDialogHeight } from "@/components/routing/useRoutingDialogHeight";

function Harness() {
  const ref = useRoutingDialogHeight();
  return (
    <div className="rt-create-dialog" style={{ maxHeight: 500 }}>
      <header className="vary-dialog-header" data-height="48" />
      <div className="vary-dialog-body" style={{ padding: "14px 18px" }}>
        <form ref={ref} data-height="200.4" />
      </div>
      <footer className="vary-dialog-footer" data-height="48" />
    </div>
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("routing dialog height", () => {
  it("tracks content after StrictMode effect replay, rounds fractional pixels and clamps to the dialog limit", () => {
    const observations: {
      callback: ResizeObserverCallback;
      target?: Element;
      disconnected: boolean;
    }[] = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observation: (typeof observations)[number];
        constructor(callback: ResizeObserverCallback) {
          this.observation = { callback, disconnected: false };
          observations.push(this.observation);
        }
        observe(target: Element) {
          this.observation.target = target;
        }
        disconnect() {
          this.observation.disconnected = true;
        }
      },
    );
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(
      function (this: Element) {
        return {
          height: Number(this.getAttribute("data-height") ?? 0),
        } as DOMRect;
      },
    );
    const view = render(
      <StrictMode>
        <Harness />
      </StrictMode>,
    );
    const dialog = view.container.firstElementChild as HTMLElement;
    const form = view.container.querySelector("form")!;
    const observation = observations.find(
      (entry) => !entry.disconnected && entry.target === form,
    )!;
    expect(observation).toBeDefined();
    expect(dialog.style.height).toBe("325px");
    act(() => {
      form.setAttribute("data-height", "600");
      observation.callback([], {} as ResizeObserver);
    });
    expect(dialog.style.height).toBe("500px");
    act(() => {
      form.setAttribute("data-height", "120");
      observation.callback([], {} as ResizeObserver);
    });
    expect(dialog.style.height).toBe("244px");
    view.unmount();
    expect(observation.disconnected).toBe(true);
  });
});
