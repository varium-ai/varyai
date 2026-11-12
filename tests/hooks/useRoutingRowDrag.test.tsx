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
function TableFixture({ drop }: { drop: (from: string, to: string) => void }) {
  const drag = useRoutingRowDrag("data-routing-group", drop);
  return (
    <div className="vary-routing" style={{ width: 700 }}>
      <div
        data-testid="table-scroll"
        className="rt-group-table-scroll"
        style={{ overflowX: "auto", overflowY: "auto" }}
      >
        <table className="rt-group-table">
          <tbody data-routing-list>
            {["a", "b", "c"].map((id) => (
              <tr key={id} data-routing-group={id} data-testid={id}>
                <td>
                  <button
                    id={`handle-${id}`}
                    onPointerDown={(event) => drag.start(event, id)}
                  >
                    {id}
                  </button>
                  <h4>{id} full wrapping name</h4>
                </td>
                <td>
                  <button>Action {id}</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <output>
        {drag.drag ? `${drag.drag.from}:${drag.drag.to}` : "idle"}
      </output>
    </div>
  );
}
function measureTable() {
  measureRows();
  const viewport = screen.getByTestId("table-scroll");
  Object.defineProperties(viewport, {
    clientWidth: { configurable: true, value: 300 },
    clientHeight: { configurable: true, value: 180 },
    scrollHeight: { configurable: true, value: 180 },
  });
  viewport.scrollLeft = 80;
  vi.spyOn(viewport, "getBoundingClientRect").mockReturnValue({
    left: 20,
    top: 0,
    right: 320,
    bottom: 180,
    width: 300,
    height: 180,
  } as DOMRect);
  vi.spyOn(
    viewport.querySelector("table")!,
    "getBoundingClientRect",
  ).mockReturnValue({ width: 500 } as DOMRect);
  for (const id of ["a", "b", "c"]) {
    for (const [i, cell] of Array.from(
      screen.getByTestId(id).children,
    ).entries())
      vi.spyOn(cell, "getBoundingClientRect").mockReturnValue({
        width: i ? 100 : 400,
      } as DOMRect);
  }
  return viewport;
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
  Object.defineProperties(scroll, {
    clientHeight: { configurable: true, value: 150 },
    scrollHeight: { configurable: true, value: 300 },
  });
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

it("lifts a complete inert table preview without moving live table rows or changing column widths", () => {
  const drop = vi.fn();
  render(<TableFixture drop={drop} />);
  const viewport = measureTable();
  const handle = screen.getByRole("button", { name: "a" });
  pointer(handle, "pointerdown", 30, 10);
  pointer(handle, "pointermove", 30, 160);
  paint();
  const preview = document.querySelector<HTMLElement>(
    "[data-routing-drag-preview]",
  );
  expect(preview).not.toBeNull();
  expect(preview).toHaveAttribute("aria-hidden", "true");
  expect(preview).toHaveAttribute("inert");
  expect(viewport).not.toContainElement(preview);
  expect(preview).toHaveTextContent("a full wrapping name");
  expect(preview).toHaveTextContent("Action a");
  expect(preview?.querySelector("[id]")).toBeNull();
  expect(preview?.querySelectorAll("td")).toHaveLength(2);
  expect(
    Array.from(preview!.querySelectorAll("col")).map(
      (cell) => cell.style.width,
    ),
  ).toEqual(["400px", "100px"]);
  expect(
    preview!.querySelector<HTMLElement>("[data-routing-drag-viewport]")
      ?.scrollLeft,
  ).toBe(80);
  for (const id of ["a", "b", "c"])
    expect(screen.getByTestId(id).style.transform).toBe("");
  expect(screen.getByTestId("a").children[0]).toHaveStyle({
    visibility: "hidden",
  });
  pointer(handle, "pointerup", 30, 160);
  expect(drop).toHaveBeenCalledExactlyOnceWith("a", "c");
  expect(document.querySelector("[data-routing-drag-preview]")).toBeNull();
  expect(
    (screen.getByTestId("a").children[0] as HTMLElement).style.visibility,
  ).toBe("");
  for (const cell of Array.from(viewport.querySelectorAll<HTMLElement>("td")))
    expect(cell.style.transform).toBe("");
});

it.each(["Escape", "pointercancel", "resize", "unmount"])(
  "removes the table preview and restores original cells on %s",
  (reason) => {
    const drop = vi.fn();
    const view = render(<TableFixture drop={drop} />);
    measureTable();
    const row = screen.getByTestId("a");
    const handle = screen.getByRole("button", { name: "a" });
    pointer(handle, "pointerdown", 30, 10);
    pointer(handle, "pointermove", 30, 160);
    paint();
    expect(
      document.querySelector("[data-routing-drag-preview]"),
    ).not.toBeNull();
    if (reason === "Escape") fireEvent.keyDown(document, { key: "Escape" });
    else if (reason === "pointercancel")
      pointer(handle, "pointercancel", 30, 160);
    else if (reason === "resize") fireEvent(window, new Event("resize"));
    else view.unmount();
    expect(document.querySelector("[data-routing-drag-preview]")).toBeNull();
    expect((row.children[0] as HTMLElement).style.visibility).toBe("");
    expect(drop).not.toHaveBeenCalled();
  },
);

it("auto-scrolls the vertical workspace instead of the horizontal-only table viewport", () => {
  const drop = vi.fn();
  render(
    <div data-testid="workspace-scroll" style={{ overflowY: "auto" }}>
      <TableFixture drop={drop} />
    </div>,
  );
  const viewport = measureTable();
  const workspace = screen.getByTestId("workspace-scroll");
  Object.defineProperties(workspace, {
    clientHeight: { configurable: true, value: 150 },
    scrollHeight: { configurable: true, value: 500 },
  });
  vi.spyOn(workspace, "getBoundingClientRect").mockReturnValue({
    top: 0,
    bottom: 150,
  } as DOMRect);
  const handle = screen.getByRole("button", { name: "a" });
  pointer(handle, "pointerdown", 30, 10);
  pointer(handle, "pointermove", 30, 149);
  paint();
  expect(workspace.scrollTop).toBeGreaterThan(0);
  expect(viewport.scrollTop).toBe(0);
  pointer(handle, "pointercancel", 30, 149);
  expect(drop).not.toHaveBeenCalled();
});
