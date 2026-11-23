import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { LegacyTitleTooltips } from "@/components/ui/legacy-title-tooltips";
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());
it("uses the themed tooltip instead of the native title and restores the original attribute on leave", () => {
  render(
    <>
      <button title="Existing hint">Fixture</button>
      <LegacyTitleTooltips />
    </>,
  );
  const owner = screen.getByRole("button", { name: "Fixture" });
  fireEvent.pointerOver(owner);
  expect(owner).not.toHaveAttribute("title");
  act(() => vi.advanceTimersByTime(400));
  expect(screen.getByRole("tooltip")).toHaveTextContent("Existing hint");
  expect(
    screen.getByRole("tooltip").closest(".vary-floating-surface"),
  ).not.toBeNull();
  fireEvent.pointerOut(owner, { relatedTarget: document.body });
  expect(owner).toHaveAttribute("title", "Existing hint");
});
it("preserves an icon control's accessible name and dismisses with Escape without reactivating native hints", () => {
  render(
    <>
      <button title="Icon action" />
      <LegacyTitleTooltips />
    </>,
  );
  const owner = screen.getByRole("button", { name: "Icon action" });
  fireEvent.focusIn(owner);
  act(() => vi.advanceTimersByTime(1));
  expect(owner).toHaveAccessibleName("Icon action");
  fireEvent.keyDown(document, { key: "Escape" });
  expect(owner).not.toHaveAttribute("title");
  fireEvent.focusOut(owner, { relatedTarget: document.body });
  expect(owner).toHaveAttribute("title", "Icon action");
  expect(owner).not.toHaveAttribute("aria-label");
});
it("tracks a changed title during hover without restoring stale content", async () => {
  const view = render(
    <>
      <button title="First hint">Fixture</button>
      <LegacyTitleTooltips />
    </>,
  );
  const owner = screen.getByRole("button", { name: "Fixture" });
  fireEvent.pointerOver(owner);
  act(() => vi.advanceTimersByTime(400));
  view.rerender(
    <>
      <button title="Updated hint">Fixture</button>
      <LegacyTitleTooltips />
    </>,
  );
  await act(async () => {
    await Promise.resolve();
  });
  expect(screen.getByRole("tooltip")).toHaveTextContent("Updated hint");
  expect(owner).not.toHaveAttribute("title");
  fireEvent.pointerOut(owner, { relatedTarget: document.body });
  expect(owner).toHaveAttribute("title", "Updated hint");
});
