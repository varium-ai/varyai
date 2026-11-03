import { fireEvent } from "@testing-library/react";
import { vi } from "vitest";

// jsdom has no animation engine; expose the live CSS state Radix Presence reads.
export function mockDialogAnimationStyles() {
  const readStyle = window.getComputedStyle.bind(window);
  vi.stubGlobal("CSS", { ...globalThis.CSS, escape: (value: string) => value });
  return vi.spyOn(window, "getComputedStyle").mockImplementation((element) => {
    const style = readStyle(element);
    if (!element.classList.contains("vary-dialog-motion")) return style;
    Object.defineProperty(style, "animationName", {
      get: () =>
        element.getAttribute("data-state") === "closed"
          ? "vary-dialog-out"
          : "vary-dialog-in",
    });
    return style;
  });
}

export function finishDialogAnimation(element: HTMLElement) {
  const event = new Event("animationend", { bubbles: true });
  Object.defineProperty(event, "animationName", { value: "vary-dialog-out" });
  fireEvent(element, event);
}
