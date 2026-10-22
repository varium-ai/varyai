// @vitest-environment jsdom
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PrimaryNavigation } from "./PrimaryNavigation";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("PrimaryNavigation", () => {
  const positions: Record<string, { left: number; width: number }> = {
    "primaryNav.home": { left: 0, width: 64 },
    "primaryNav.security": { left: 68, width: 88 },
    "primaryNav.stats": { left: 160, width: 64 },
    "primaryNav.settings": { left: 228, width: 64 },
  };
  const createObserver = vi.fn();
  let resize: ResizeObserverCallback;
  let motionChange: () => void;
  let reducedMotion: boolean;
  let inFlight: { left: number; width: number } | null;

  beforeEach(() => {
    reducedMotion = false;
    inFlight = null;
    vi.useFakeTimers();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: ResizeObserverCallback) {
          createObserver();
          resize = callback;
        }
        observe() {}
        disconnect() {}
      },
    );
    vi.stubGlobal("matchMedia", () => ({
      get matches() {
        return reducedMotion;
      },
      addEventListener: (_event: string, callback: () => void) => {
        motionChange = callback;
      },
      removeEventListener: vi.fn(),
    }));
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      function (this: HTMLElement) {
        const position = positions[this.textContent ?? ""];
        const isIndicator = this.dataset.testid === "primary-nav-indicator";
        const isUnderline = this.dataset.testid === "primary-nav-underline";
        const left = isIndicator
          ? (inFlight?.left ?? parseFloat(this.style.left || "0"))
          : isUnderline
            ? parseFloat(this.style.left || "0")
            : (position?.left ?? 0);
        const width = isIndicator
          ? (inFlight?.width ?? parseFloat(this.style.width || "0"))
          : isUnderline
            ? parseFloat(this.style.width || "0")
            : (position?.width ?? 292);
        return {
          x: left + 40,
          y: 0,
          left: left + 40,
          right: left + width + 40,
          top: 0,
          bottom: 32,
          width,
          height: 32,
          toJSON() {},
        };
      },
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("stretches one shared selection pill across differently sized tabs in either direction", () => {
    const { rerender } = render(
      <PrimaryNavigation activeTab="home" onSelect={vi.fn()} />,
    );
    const indicator = screen.getByTestId("primary-nav-indicator");
    const underline = screen.getByTestId("primary-nav-underline");
    expect(indicator).toHaveAttribute("aria-hidden", "true");
    expect(indicator).toHaveStyle({
      width: "64px",
      left: "0px",
    });
    expect(indicator).toHaveAttribute("data-phase", "idle");
    expect(underline).toHaveStyle({ left: "16px", width: "32px" });
    expect(underline).toHaveAttribute("data-phase", "idle");

    rerender(<PrimaryNavigation activeTab="security" onSelect={vi.fn()} />);
    expect(screen.getAllByTestId("primary-nav-indicator")).toHaveLength(1);
    expect(indicator).toHaveAttribute("data-phase", "stretch");
    expect(indicator).toHaveStyle({ left: "0px", width: "156px" });
    expect(underline).toHaveAttribute("data-phase", "slide");
    expect(underline).toHaveStyle({ left: "96px", width: "32px" });

    act(() => vi.advanceTimersByTime(170));
    expect(indicator).toHaveAttribute("data-phase", "settle");
    expect(indicator).toHaveStyle({ left: "68px", width: "88px" });
    expect(underline).toHaveAttribute("data-phase", "slide");
    expect(underline).toHaveStyle({ left: "96px", width: "32px" });

    act(() => vi.advanceTimersByTime(280));
    expect(indicator).toHaveAttribute("data-phase", "idle");
    expect(underline).toHaveAttribute("data-phase", "idle");

    rerender(<PrimaryNavigation activeTab="home" onSelect={vi.fn()} />);
    expect(indicator).toHaveAttribute("data-phase", "stretch");
    expect(indicator).toHaveStyle({ left: "0px", width: "156px" });
    expect(underline).toHaveStyle({ left: "16px", width: "32px" });
    act(() => vi.advanceTimersByTime(170));
    expect(indicator).toHaveStyle({ left: "0px", width: "64px" });
    expect(underline).toHaveStyle({ left: "16px", width: "32px" });
    expect(createObserver).toHaveBeenCalledTimes(1);
  });

  it("redirects a rapid switch from the pill's visible position", () => {
    const { rerender } = render(
      <PrimaryNavigation activeTab="home" onSelect={vi.fn()} />,
    );
    rerender(<PrimaryNavigation activeTab="settings" onSelect={vi.fn()} />);
    inFlight = { left: 20, width: 190 };
    rerender(<PrimaryNavigation activeTab="stats" onSelect={vi.fn()} />);
    const indicator = screen.getByTestId("primary-nav-indicator");
    const underline = screen.getByTestId("primary-nav-underline");
    expect(indicator).toHaveAttribute("data-phase", "stretch");
    expect(indicator).toHaveStyle({ left: "20px", width: "204px" });
    expect(underline).toHaveStyle({ left: "176px", width: "32px" });
    act(() => vi.advanceTimersByTime(170));
    expect(indicator).toHaveStyle({ left: "160px", width: "64px" });
    expect(underline).toHaveStyle({ left: "176px", width: "32px" });
  });

  it("keeps the pill's rounded edge in sync during a jump to the last tab", () => {
    const { rerender } = render(
      <PrimaryNavigation activeTab="home" onSelect={vi.fn()} />,
    );
    rerender(<PrimaryNavigation activeTab="settings" onSelect={vi.fn()} />);
    const indicator = screen.getByTestId("primary-nav-indicator");
    const underline = screen.getByTestId("primary-nav-underline");
    expect(indicator).toHaveStyle({ left: "0px", width: "292px" });
    expect(underline).toHaveStyle({ left: "244px", width: "32px" });
    act(() => vi.advanceTimersByTime(170));
    expect(indicator).toHaveStyle({ left: "228px", width: "64px" });
    expect(underline).toHaveStyle({ left: "244px", width: "32px" });
  });

  it("reserves room for rounded ends and focus rings without adding a hover pill", () => {
    render(<PrimaryNavigation activeTab="home" onSelect={vi.fn()} />);
    expect(screen.getByRole("navigation")).toHaveClass("p-1");
    for (const button of screen
      .getByRole("navigation")
      .querySelectorAll("button")) {
      expect(button.className).not.toMatch(/(?:dark:)?hover:bg-/);
    }
  });

  it("does not let resize alignment cancel an active transition", () => {
    const { rerender } = render(
      <PrimaryNavigation activeTab="home" onSelect={vi.fn()} />,
    );
    rerender(<PrimaryNavigation activeTab="settings" onSelect={vi.fn()} />);
    const indicator = screen.getByTestId("primary-nav-indicator");
    const underline = screen.getByTestId("primary-nav-underline");
    act(() => resize([], {} as ResizeObserver));
    expect(indicator).toHaveAttribute("data-phase", "stretch");
    expect(indicator).toHaveStyle({ left: "0px", width: "292px" });
    expect(underline).toHaveAttribute("data-phase", "slide");
    expect(underline).toHaveStyle({ left: "244px", width: "32px" });
  });

  it("aligns immediately when reduced motion is enabled", () => {
    const { rerender } = render(
      <PrimaryNavigation activeTab="home" onSelect={vi.fn()} />,
    );
    reducedMotion = true;
    rerender(<PrimaryNavigation activeTab="settings" onSelect={vi.fn()} />);
    const indicator = screen.getByTestId("primary-nav-indicator");
    const underline = screen.getByTestId("primary-nav-underline");
    expect(indicator).toHaveStyle({
      width: "64px",
      left: "228px",
    });
    expect(underline).toHaveStyle({ width: "32px", left: "244px" });
    expect(indicator).toHaveAttribute("data-phase", "idle");
    positions["primaryNav.settings"].width = 96;
    act(() => resize([], {} as ResizeObserver));
    expect(indicator).toHaveStyle({ width: "96px" });
    expect(underline).toHaveStyle({ width: "32px", left: "260px" });
    positions["primaryNav.settings"].width = 64;

    act(() => motionChange());
  });

  it("hides the pill and underline when no primary page is active", () => {
    const { rerender } = render(
      <PrimaryNavigation activeTab="home" onSelect={vi.fn()} />,
    );
    rerender(<PrimaryNavigation activeTab={null} onSelect={vi.fn()} />);
    expect(screen.getByTestId("primary-nav-indicator")).not.toBeVisible();
    expect(screen.getByTestId("primary-nav-underline")).not.toBeVisible();
    rerender(<PrimaryNavigation activeTab="stats" onSelect={vi.fn()} />);
    expect(screen.getByTestId("primary-nav-indicator")).toBeVisible();
    expect(screen.getByTestId("primary-nav-underline")).toBeVisible();
    expect(screen.getByTestId("primary-nav-indicator")).toHaveAttribute(
      "data-phase",
      "idle",
    );
  });

  it("shows the active section and routes each design tab", () => {
    const onSelect = vi.fn();
    const { rerender } = render(
      <PrimaryNavigation activeTab="home" onSelect={onSelect} />,
    );
    expect(screen.getByRole("button", { name: "app.title" })).toHaveTextContent(
      "Vary AI",
    );
    expect(screen.getByText("AI")).toHaveClass("text-primary");
    expect(
      screen.getByRole("button", { name: "primaryNav.home" }),
    ).toHaveAttribute("aria-current", "page");
    expect(screen.getAllByTestId("primary-nav-underline")).toHaveLength(1);
    fireEvent.click(
      screen.getByRole("button", { name: "primaryNav.security" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "primaryNav.stats" }));
    fireEvent.click(
      screen.getByRole("button", { name: "primaryNav.settings" }),
    );
    expect(onSelect.mock.calls.map(([tab]) => tab)).toEqual([
      "security",
      "stats",
      "settings",
    ]);

    rerender(<PrimaryNavigation activeTab="security" onSelect={onSelect} />);
    expect(
      screen.getByRole("button", { name: "primaryNav.security" }),
    ).toHaveAttribute("aria-current", "page");
    expect(screen.getAllByTestId("primary-nav-underline")).toHaveLength(1);
  });

  it("keeps navigation locked while another page has pending edits", () => {
    const onSelect = vi.fn();
    render(<PrimaryNavigation activeTab={null} onSelect={onSelect} disabled />);
    fireEvent.click(screen.getByRole("button", { name: "primaryNav.home" }));
    fireEvent.click(screen.getByRole("button", { name: "app.title" }));
    expect(onSelect).not.toHaveBeenCalled();
  });
});
