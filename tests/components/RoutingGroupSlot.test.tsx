import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RoutingGroupSlot } from "@/components/routing/RoutingGroupSlot";

describe("RoutingGroupSlot", () => {
  let height = 80;
  let reducedMotion = false;
  let listener: ((event: MediaQueryListEvent) => void) | undefined;
  const animations: {
    target: Element;
    cancel: ReturnType<typeof vi.fn>;
    onfinish: (() => void) | null;
  }[] = [];
  const animate = vi.fn(function (this: Element) {
    const animation = {
      target: this,
      cancel: vi.fn(),
      onfinish: null as (() => void) | null,
    };
    animations.push(animation);
    return animation as unknown as Animation;
  });
  const originalAnimate = Object.getOwnPropertyDescriptor(
    Element.prototype,
    "animate",
  );

  beforeEach(() => {
    height = 80;
    reducedMotion = false;
    listener = undefined;
    animations.length = 0;
    animate.mockClear();
    Object.defineProperty(Element.prototype, "animate", {
      configurable: true,
      value: animate,
    });
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(
      () => ({ height, top: 0, bottom: height }) as DOMRect,
    );
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({
        get matches() {
          return reducedMotion;
        },
        addEventListener: (_name: string, callback: typeof listener) => {
          listener = callback;
        },
        removeEventListener: vi.fn(),
      })),
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    if (originalAnimate)
      Object.defineProperty(Element.prototype, "animate", originalAnimate);
    else Reflect.deleteProperty(Element.prototype, "animate");
  });

  function slot(expanded: boolean, onCollapseComplete = vi.fn()) {
    return (
      <ul>
        <RoutingGroupSlot
          expanded={expanded}
          editor={
            expanded ? (
              <form aria-label="Editor">
                <header>Editor heading</header>
                <fieldset>
                  <label>
                    Name
                    <input aria-label="Name" />
                  </label>
                  <p>Member settings</p>
                </fieldset>
              </form>
            ) : null
          }
          onCollapseComplete={onCollapseComplete}
        >
          <button>Edit</button>
        </RoutingGroupSlot>
      </ul>
    );
  }
  function heights(from: number, to: number) {
    vi.mocked(Element.prototype.getBoundingClientRect)
      .mockReturnValueOnce({ height: from, top: 0, bottom: from } as DOMRect)
      .mockReturnValue({ height: to, top: 0, bottom: to } as DOMRect);
  }
  function heightAnimation() {
    return [...animations]
      .reverse()
      .find((animation) => animation.target.tagName === "LI")!;
  }
  function finishHeight() {
    act(() => heightAnimation().onfinish?.());
  }

  it("uses the reference implementation's damped spring and immediately releases the editor on fold", () => {
    const complete = vi.fn();
    const panel = render(slot(false, complete));
    heights(80, 420);
    panel.rerender(slot(true, complete));
    const editor = screen.getByRole("form", { name: "Editor" });
    expect(animate).toHaveBeenCalledWith(
      [{ height: "80px" }, { height: "420px" }],
      expect.objectContaining({
        duration: 570,
        easing: "cubic-bezier(.2, .9, .25, 1.02)",
      }),
    );
    finishHeight();
    heights(420, 80);
    panel.rerender(slot(false, complete));
    expect(editor).not.toBeInTheDocument();
    expect(animate).toHaveBeenCalledWith(
      [{ height: "420px" }, { height: "80px" }],
      expect.objectContaining({
        duration: 420,
        easing: "cubic-bezier(.32, .72, 0, 1)",
      }),
    );
    expect(screen.getByRole("listitem")).toHaveStyle({ overflow: "hidden" });
    expect(complete).not.toHaveBeenCalled();
    finishHeight();
    expect(complete).toHaveBeenCalledOnce();
    expect(screen.getByRole("listitem").style.overflow).toBe("");
    expect(screen.getByRole("button", { name: "Edit" })).toBeEnabled();
  });

  it("reveals the heading and form sections in sequence and lets their entrance finish", () => {
    const panel = render(slot(false));
    heights(80, 420);
    panel.rerender(slot(true));
    const contentAnimations = animations.filter(
      (a) => a.target.tagName !== "LI",
    );
    expect(contentAnimations.map((a) => a.target.tagName)).toEqual([
      "HEADER",
      "LABEL",
      "P",
    ]);
    for (const [index] of contentAnimations.entries()) {
      expect(animate).toHaveBeenCalledWith(
        [
          { opacity: 0, transform: "translateY(-6px)" },
          { opacity: 1, transform: "none" },
        ],
        {
          duration: 420,
          delay: 40 + index * 18,
          easing: "cubic-bezier(.32, .72, 0, 1)",
          fill: "backwards",
        },
      );
    }
    finishHeight();
    contentAnimations.forEach((a) => expect(a.cancel).not.toHaveBeenCalled());
    heights(420, 80);
    panel.rerender(slot(false));
    contentAnimations.forEach((a) => expect(a.cancel).toHaveBeenCalledOnce());
    expect(animate).toHaveBeenCalledWith([{ opacity: 0 }, { opacity: 1 }], {
      duration: 300,
      delay: 90,
      easing: "ease-out",
      fill: "backwards",
    });
  });

  it("uses the sampled spring when CSS linear easing is supported", () => {
    vi.stubGlobal("CSS", { supports: vi.fn(() => true) });
    height = 420;
    render(slot(true));
    expect(animate).toHaveBeenCalledWith(
      [{ height: "0px" }, { height: "420px" }],
      expect.objectContaining({
        easing: expect.stringMatching(/^linear\(0, 0\.0203,.*1\.0151.*1\)$/),
      }),
    );
  });

  it("starts a new editor from zero and restarts interruptions from the visible height", () => {
    height = 420;
    const panel = render(slot(true));
    expect(animate).toHaveBeenCalledWith(
      [{ height: "0px" }, { height: "420px" }],
      expect.objectContaining({ duration: 570 }),
    );
    const opening = [...animations];
    heights(230, 80);
    panel.rerender(slot(false));
    opening.forEach((animation) =>
      expect(animation.cancel).toHaveBeenCalledOnce(),
    );
    expect(animate).toHaveBeenCalledWith(
      [{ height: "230px" }, { height: "80px" }],
      expect.objectContaining({ duration: 420 }),
    );
    const closing = animations.slice(opening.length);
    heights(160, 420);
    panel.rerender(slot(true));
    closing.forEach((animation) =>
      expect(animation.cancel).toHaveBeenCalledOnce(),
    );
    expect(animate).toHaveBeenCalledWith(
      [{ height: "160px" }, { height: "420px" }],
      expect.objectContaining({ duration: 570 }),
    );
    const running = animations.slice(opening.length + closing.length);
    panel.unmount();
    running.forEach((animation) =>
      expect(animation.cancel).toHaveBeenCalledOnce(),
    );
  });

  it("removes outgoing content immediately for reduced motion, including preference changes", () => {
    reducedMotion = true;
    const complete = vi.fn();
    const panel = render(slot(true, complete));
    expect(animate).not.toHaveBeenCalled();
    panel.rerender(slot(false, complete));
    expect(
      screen.queryByRole("form", { hidden: true }),
    ).not.toBeInTheDocument();
    expect(complete).toHaveBeenCalledOnce();
    reducedMotion = false;
    heights(80, 420);
    panel.rerender(slot(true, complete));
    finishHeight();
    heights(420, 80);
    panel.rerender(slot(false, complete));
    const running = animations.filter(
      (animation) => !animation.cancel.mock.calls.length,
    );
    reducedMotion = true;
    act(() => listener?.({ matches: true } as MediaQueryListEvent));
    running.forEach((animation) => expect(animation.cancel).toHaveBeenCalled());
    expect(
      screen.queryByRole("form", { hidden: true }),
    ).not.toBeInTheDocument();
    expect(complete).toHaveBeenCalledTimes(2);
  });

  it("does not replay animation for field edits or unchanged rows", () => {
    const panel = render(slot(false));
    panel.rerender(slot(false));
    expect(animate).not.toHaveBeenCalled();
    heights(80, 420);
    panel.rerender(slot(true));
    const calls = animate.mock.calls.length;
    panel.rerender(slot(true));
    expect(animate).toHaveBeenCalledTimes(calls);
  });

  it("falls back to usable content if height or content animation cannot run", () => {
    const complete = vi.fn();
    const panel = render(slot(false, complete));
    Reflect.deleteProperty(Element.prototype, "animate");
    panel.rerender(slot(true, complete));
    expect(screen.getByRole("form")).toBeInTheDocument();
    Object.defineProperty(Element.prototype, "animate", {
      configurable: true,
      value: animate,
    });
    heights(420, 80);
    animate.mockImplementationOnce(() => {
      throw new Error("Animation unavailable");
    });
    panel.rerender(slot(false, complete));
    expect(complete).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole("form", { hidden: true }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("listitem").style.overflow).toBe("");
    expect(screen.getByRole("button", { name: "Edit" })).toBeEnabled();
  });

  it("follows an offscreen editor upward in step with its measured folding height", () => {
    let frame: FrameRequestCallback | undefined;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frame = callback;
      return 1;
    });
    vi.spyOn(window, "cancelAnimationFrame");
    const panel = render(
      <div style={{ overflowY: "auto" }} data-testid="scroll">
        <div style={{ height: 600 }} />
        {slot(true)}
      </div>,
    );
    finishHeight();
    const scroll = screen.getByTestId("scroll");
    const row = screen.getByRole("listitem");
    scroll.scrollTop = 500;
    let measurements = 0;
    vi.mocked(Element.prototype.getBoundingClientRect).mockImplementation(
      function (this: Element) {
        if (this === scroll)
          return { top: 100, height: 400, bottom: 500 } as DOMRect;
        const measuredHeight =
          this === row ? ([420, 80][measurements++] ?? height) : 80;
        return {
          top: -200,
          height: measuredHeight,
          bottom: measuredHeight - 200,
        } as DOMRect;
      },
    );
    height = 420;
    panel.rerender(
      <div style={{ overflowY: "auto" }} data-testid="scroll">
        <div style={{ height: 600 }} />
        {slot(false)}
      </div>,
    );
    height = 250;
    act(() => frame?.(1));
    expect(scroll.scrollTop).toBe(350);
    expect(scroll.style.overflowAnchor).toBe("none");
    finishHeight();
    expect(scroll.scrollTop).toBe(200);
    expect(window.cancelAnimationFrame).toHaveBeenCalled();
    expect(scroll.style.overflowAnchor).toBe("");
  });

  it("lets a deliberate user scroll interrupt the automatic fold-follow", () => {
    let frame: FrameRequestCallback | undefined;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      frame = callback;
      return 1;
    });
    const panel = render(
      <div style={{ overflowY: "auto" }} data-testid="scroll">
        {slot(true)}
      </div>,
    );
    finishHeight();
    const scroll = screen.getByTestId("scroll");
    const row = screen.getByRole("listitem");
    scroll.scrollTop = 500;
    let measurements = 0;
    vi.mocked(Element.prototype.getBoundingClientRect).mockImplementation(
      function (this: Element) {
        return {
          top: this === scroll ? 100 : -200,
          height: this === row ? ([420, 80][measurements++] ?? 250) : 80,
        } as DOMRect;
      },
    );
    panel.rerender(
      <div style={{ overflowY: "auto" }} data-testid="scroll">
        {slot(false)}
      </div>,
    );
    fireEvent.wheel(scroll);
    scroll.scrollTop = 475;
    act(() => frame?.(1));
    finishHeight();
    expect(scroll.scrollTop).toBe(475);
  });

  it("does not scroll a visible row or report the same collapse twice on preference changes", () => {
    const request = vi.spyOn(window, "requestAnimationFrame");
    const complete = vi.fn();
    const panel = render(
      <div style={{ overflowY: "auto" }}>{slot(true, complete)}</div>,
    );
    finishHeight();
    request.mockClear();
    heights(420, 80);
    panel.rerender(
      <div style={{ overflowY: "auto" }}>{slot(false, complete)}</div>,
    );
    expect(request).not.toHaveBeenCalled();
    finishHeight();
    reducedMotion = true;
    act(() => listener?.({ matches: true } as MediaQueryListEvent));
    expect(complete).toHaveBeenCalledOnce();
  });
});
