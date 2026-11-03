import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { RoutingTrace } from "@/lib/api/routingTrace";
import { RoutingPackets } from "@/components/routing/RoutingPackets";

const request: RoutingTrace = {
  id: "synthetic-flight",
  startedAt: Date.now(),
  agent: "claude-desktop",
  model: "synthetic/model",
  groupName: "Synthetic",
  strategy: "ordered",
  state: "routing",
  durationMs: 10,
  candidates: [
    { providerId: "a", providerName: "Synthetic A", model: "a" },
    { providerId: "b", providerName: "Synthetic B", model: "b" },
  ],
  steps: [{ kind: "attempt", atMs: 5, candidate: 0 }],
};
const paths = ["M0,0 L100,0", "M100,0 L200,0", "M100,0 L200,100"];
let callbacks: Map<number, FrameRequestCallback>;
let frameId: number;
let now: number;
let hidden = false;
let reduced = false;
let motionChange: ((event: MediaQueryListEvent) => void) | undefined;

function frame(elapsed: number) {
  now += elapsed;
  act(() => {
    const pending = [...callbacks.values()];
    callbacks.clear();
    pending.forEach((callback) => callback(now));
  });
}
function finishFlights() {
  for (let count = 0; count < 12; count++) frame(400);
}
function point(container: HTMLElement) {
  return container.querySelector(".rt-packet");
}
beforeEach(() => {
  callbacks = new Map();
  frameId = 0;
  now = 0;
  hidden = false;
  reduced = false;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callbacks.set(++frameId, callback);
    return frameId;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => callbacks.delete(id));
  vi.spyOn(document, "hidden", "get").mockImplementation(() => hidden);
  vi.stubGlobal("matchMedia", () => ({
    matches: reduced,
    addEventListener: (_name: string, listener: typeof motionChange) => {
      motionChange = listener;
    },
    removeEventListener: vi.fn(),
  }));
  Object.defineProperty(SVGElement.prototype, "getTotalLength", {
    configurable: true,
    value: () => 100,
  });
  Object.defineProperty(SVGElement.prototype, "getPointAtLength", {
    configurable: true,
    value(this: SVGElement, distance: number) {
      const path = this.getAttribute("d");
      return {
        x: path?.startsWith("M100") ? 100 + distance : distance,
        y: path?.endsWith("100") ? distance : 0,
      };
    },
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  delete (SVGElement.prototype as Partial<SVGPathElement>).getTotalLength;
  delete (SVGElement.prototype as Partial<SVGPathElement>).getPointAtLength;
});

it("flies a glowing dot through both wires, holds for the real response, and returns after completion", () => {
  const view = render(<RoutingPackets row={request} paths={paths} active />);
  frame(0);
  frame(300);
  expect(point(view.container)?.getAttribute("transform")).toMatch(
    /translate\((?!0 )/,
  );
  expect(view.container.querySelector(".rt-flier,.wing,.body")).toBeNull();
  finishFlights();
  expect(point(view.container)).toHaveClass("held");
  expect(point(view.container)).toHaveAttribute(
    "transform",
    "translate(200 0)",
  );
  const held = point(view.container);
  view.rerender(
    <RoutingPackets
      row={{ ...request, durationMs: 2000 }}
      paths={paths}
      active
    />,
  );
  expect(point(view.container)).toBe(held);
  expect(callbacks.size).toBe(0);
  view.rerender(
    <RoutingPackets
      row={{
        ...request,
        state: "completed",
        status: 200,
        steps: [
          ...request.steps,
          { kind: "completed", atMs: 2100, candidate: 0 },
        ],
      }}
      paths={paths}
      active
    />,
  );
  frame(0);
  frame(300);
  expect(point(view.container)).toHaveClass("back");
  expect(point(view.container)).not.toHaveAttribute(
    "transform",
    "translate(200 0)",
  );
  finishFlights();
  expect(point(view.container)).toHaveAttribute("visibility", "hidden");
  expect(callbacks.size).toBe(0);
});

it("shows fast failed requests first observed after completion, without replaying them on every poll", () => {
  const failed: RoutingTrace = {
    ...request,
    state: "failed",
    status: 502,
    steps: [...request.steps, { kind: "failed", atMs: 40, status: 502 }],
  };
  const view = render(<RoutingPackets row={failed} paths={paths} active />);
  frame(0);
  frame(700);
  expect(point(view.container)).toHaveAttribute("visibility", "visible");
  frame(800);
  expect(point(view.container)).toHaveClass("back", "err");
  finishFlights();
  view.rerender(<RoutingPackets row={{ ...failed }} paths={paths} active />);
  expect(point(view.container)).toHaveAttribute("visibility", "hidden");
  expect(callbacks.size).toBe(0);
});

it("returns a rejected attempt to the hub before moving to the next recorded candidate", () => {
  const view = render(<RoutingPackets row={request} paths={paths} active />);
  finishFlights();
  view.rerender(
    <RoutingPackets
      row={{
        ...request,
        steps: [
          ...request.steps,
          { kind: "rejected", atMs: 100, candidate: 0, status: 429 },
          { kind: "attempt", atMs: 101, candidate: 1 },
        ],
      }}
      paths={paths}
      active
    />,
  );
  frame(0);
  frame(300);
  expect(point(view.container)).toHaveClass("back", "err");
  expect(point(view.container)?.getAttribute("transform")).not.toBe(
    "translate(200 100)",
  );
  finishFlights();
  expect(point(view.container)).toHaveClass("held");
  expect(point(view.container)).not.toHaveClass("err");
  expect(point(view.container)).toHaveAttribute(
    "transform",
    "translate(200 100)",
  );
});

it("keeps historical selections still, while explicit replay follows only the supplied steps", () => {
  const old = {
    ...request,
    startedAt: Date.now() - 60000,
    state: "completed" as const,
  };
  const view = render(
    <RoutingPackets row={old} paths={paths} active={false} />,
  );
  expect(callbacks.size).toBe(0);
  expect(point(view.container)).toHaveAttribute("visibility", "hidden");
  view.rerender(
    <RoutingPackets
      row={{ ...old, state: "routing" }}
      paths={paths}
      active
      replay
    />,
  );
  finishFlights();
  expect(point(view.container)).toHaveClass("held");
  view.rerender(
    <RoutingPackets
      row={{ ...old, state: "routing", steps: [] }}
      paths={paths}
      active
      replay
    />,
  );
  finishFlights();
  expect(point(view.container)).toHaveAttribute(
    "transform",
    "translate(100 0)",
  );
});

it("tracks new wire geometry while held and cancels all work when hidden, reduced, or unmounted", () => {
  const view = render(<RoutingPackets row={request} paths={paths} active />);
  finishFlights();
  view.rerender(
    <RoutingPackets
      row={request}
      paths={["M0,0 L100,0", "M100,0 L200,100"]}
      active
    />,
  );
  expect(point(view.container)).toHaveAttribute(
    "transform",
    "translate(200 100)",
  );
  view.rerender(
    <RoutingPackets row={{ ...request, id: "next" }} paths={paths} active />,
  );
  act(() => {
    hidden = true;
    document.dispatchEvent(new Event("visibilitychange"));
  });
  expect(callbacks.size).toBe(0);
  act(() => {
    hidden = false;
    document.dispatchEvent(new Event("visibilitychange"));
  });
  expect(callbacks.size).toBe(1);
  act(() => {
    reduced = true;
    motionChange?.({ matches: true } as MediaQueryListEvent);
  });
  expect(callbacks.size).toBe(0);
  expect(point(view.container)).toHaveAttribute("visibility", "hidden");
  view.unmount();
  expect(callbacks.size).toBe(0);
});

it("does not schedule motion before layout or when reduced motion was already requested", () => {
  reduced = true;
  const view = render(<RoutingPackets row={request} paths={paths} active />);
  expect(callbacks.size).toBe(0);
  view.rerender(<RoutingPackets row={request} paths={[]} active />);
  expect(point(view.container)).toHaveAttribute("visibility", "hidden");
});
