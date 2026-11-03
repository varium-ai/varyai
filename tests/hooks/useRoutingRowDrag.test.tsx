import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useRoutingRowDrag } from "@/hooks/useRoutingRowDrag";

function Fixture({ drop }: { drop: (from: string, to: string) => void }) {
  const drag = useRoutingRowDrag("data-routing-group", drop);
  return (
    <div data-routing-list>
      {["a", "b", "c"].map((id) => (
        <div key={id}>
          <div data-routing-group={id} data-testid={id}>
            <button
              onPointerDown={(event) => drag.start(event, id)}
              onClick={() => {
                if (!drag.consumeClick(id)) drop("menu", id);
              }}
            >
              {id}
            </button>
          </div>
        </div>
      ))}
      <output>
        {drag.drag ? `${drag.drag.from}:${drag.drag.to}` : "idle"}
      </output>
    </div>
  );
}
function pointer(target: Element, type: string, x: number, y: number) {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    clientX: x,
    clientY: y,
    button: 0,
  });
  Object.defineProperties(event, {
    pointerId: { value: 7 },
    isPrimary: { value: true },
  });
  fireEvent(target, event);
}
let frame: FrameRequestCallback | null = null;
beforeEach(() => {
  frame = null;
  vi.stubGlobal(
    "requestAnimationFrame",
    vi.fn((callback) => {
      frame = callback;
      return 1;
    }),
  );
  vi.stubGlobal(
    "cancelAnimationFrame",
    vi.fn(() => {
      frame = null;
    }),
  );
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
function measureRows() {
  for (const [i, id] of ["a", "b", "c"].entries()) {
    vi.spyOn(screen.getByTestId(id), "getBoundingClientRect").mockReturnValue({
      top: i * 60,
      bottom: (i + 1) * 60,
      left: 0,
      right: 300,
      width: 300,
      height: 60,
      x: 0,
      y: i * 60,
      toJSON() {},
    });
  }
}
function paint() {
  act(() => {
    const callback = frame;
    frame = null;
    callback?.(16);
  });
}

it("moves across nested row wrappers only after the pointer threshold and suppresses the drag-ending menu click", () => {
  const drop = vi.fn();
  render(<Fixture drop={drop} />);
  measureRows();
  const handle = screen.getByRole("button", { name: "a" });
  pointer(handle, "pointerdown", 10, 10);
  pointer(handle, "pointermove", 12, 12);
  expect(screen.getByText("idle")).toBeInTheDocument();
  pointer(handle, "pointermove", 10, 160);
  paint();
  expect(screen.getByText("a:c")).toBeInTheDocument();
  expect(screen.getByTestId("a").parentElement).toHaveStyle({
    transform: "translateY(120px)",
  });
  expect(screen.getByTestId("b").parentElement).toHaveStyle({
    transform: "translateY(-60px)",
  });
  pointer(handle, "pointerup", 10, 160);
  expect(drop).toHaveBeenCalledExactlyOnceWith("a", "c");
  expect(screen.getByTestId("a").parentElement?.style.transform).toBe("");
  fireEvent.click(handle);
  expect(drop).toHaveBeenCalledTimes(1);
  fireEvent.click(handle);
  expect(drop).toHaveBeenLastCalledWith("menu", "a");
});

it("cancels without persisting an order", () => {
  const drop = vi.fn();
  render(<Fixture drop={drop} />);
  measureRows();
  const handle = screen.getByRole("button", { name: "a" });
  pointer(handle, "pointerdown", 10, 10);
  pointer(handle, "pointermove", 10, 160);
  pointer(handle, "pointercancel", 10, 160);
  expect(drop).not.toHaveBeenCalled();
  expect(screen.getByText("idle")).toBeInTheDocument();
});

it.each(["Escape", "blur", "resize"])(
  "cancels a floating row on %s without saving",
  (reason) => {
    const drop = vi.fn();
    render(<Fixture drop={drop} />);
    measureRows();
    const handle = screen.getByRole("button", { name: "a" });
    pointer(handle, "pointerdown", 10, 10);
    pointer(handle, "pointermove", 10, 160);
    paint();
    if (reason === "Escape") fireEvent.keyDown(document, { key: "Escape" });
    else fireEvent(window, new Event(reason));
    expect(drop).not.toHaveBeenCalled();
    expect(screen.getByTestId("b").parentElement?.style.transform).toBe("");
    expect(screen.getByText("idle")).toBeInTheDocument();
  },
);

it("scrolls at the edge and stops animation frames on cancellation", () => {
  const drop = vi.fn();
  const { unmount } = render(
    <div data-testid="scroll" style={{ overflowY: "auto" }}>
      <Fixture drop={drop} />
    </div>,
  );
  measureRows();
  const scroll = screen.getByTestId("scroll");
  vi.spyOn(scroll, "getBoundingClientRect").mockReturnValue({
    top: 0,
    bottom: 150,
  } as DOMRect);
  const handle = screen.getByRole("button", { name: "a" });
  pointer(handle, "pointerdown", 10, 10);
  pointer(handle, "pointermove", 10, 149);
  paint();
  expect(scroll.scrollTop).toBeGreaterThan(0);
  unmount();
  expect(frame).toBeNull();
  expect(drop).not.toHaveBeenCalled();
});
