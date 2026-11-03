import { act, fireEvent, render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { WorkspaceNavigation } from "@/components/layout/WorkspaceNavigation";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

describe("WorkspaceNavigation", () => {
  it("moves one decorative selection only after navigation is accepted", () => {
    const position = { top: 92, width: 160, height: 28 };
    const top = vi
      .spyOn(HTMLElement.prototype, "offsetTop", "get")
      .mockImplementation(function (this: HTMLElement) {
        return this.getAttribute("aria-current") === "page" ? position.top : 0;
      });
    const width = vi
      .spyOn(HTMLElement.prototype, "offsetWidth", "get")
      .mockImplementation(() => position.width);
    const height = vi
      .spyOn(HTMLElement.prototype, "offsetHeight", "get")
      .mockImplementation(() => position.height);
    const onSelect = vi.fn();
    try {
      const { container, rerender } = render(
        <WorkspaceNavigation view="agents" onSelect={onSelect} />,
      );
      const selection = container.querySelector(".workspace-nav-selection");
      expect(selection).toHaveAttribute("aria-hidden", "true");
      expect(selection).toHaveStyle({
        transform: "translate3d(0px, 92px, 0)",
        width: "160px",
        height: "28px",
      });
      fireEvent.click(
        screen.getByRole("button", { name: "workspaceNav.routing" }),
      );
      expect(onSelect).toHaveBeenCalledWith("routing");
      expect(selection).toHaveStyle({ transform: "translate3d(0px, 92px, 0)" });
      position.top = 128;
      rerender(<WorkspaceNavigation view="routing" onSelect={onSelect} />);
      expect(container.querySelector(".workspace-nav-selection")).toBe(
        selection,
      );
      expect(selection).toHaveStyle({
        transform: "translate3d(0px, 128px, 0)",
      });
      expect(container.querySelectorAll('[aria-current="page"]')).toHaveLength(
        1,
      );
      position.top = 540;
      rerender(<WorkspaceNavigation view="settings" onSelect={onSelect} />);
      expect(selection).toHaveStyle({
        transform: "translate3d(0px, 540px, 0)",
      });
    } finally {
      top.mockRestore();
      width.mockRestore();
      height.mockRestore();
    }
  });

  it("remeasures the selection on sidebar resize and cleans up its observer", () => {
    let resized: ResizeObserverCallback | undefined;
    const disconnect = vi.fn();
    const observe = vi.fn();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: ResizeObserverCallback) {
          resized = callback;
        }
        observe = observe;
        disconnect = disconnect;
      },
    );
    let currentWidth = 160;
    const width = vi
      .spyOn(HTMLElement.prototype, "offsetWidth", "get")
      .mockImplementation(() => currentWidth);
    try {
      const { container, unmount } = render(
        <WorkspaceNavigation view="providers" onSelect={vi.fn()} />,
      );
      const selection = container.querySelector(".workspace-nav-selection");
      expect(selection).toHaveStyle({ width: "160px" });
      currentWidth = 40;
      act(() => resized?.([], {} as ResizeObserver));
      expect(selection).toHaveStyle({ width: "40px" });
      expect(observe).toHaveBeenCalledWith(container.querySelector("aside"));
      unmount();
      expect(disconnect).toHaveBeenCalledOnce();
    } finally {
      width.mockRestore();
      vi.unstubAllGlobals();
    }
  });

  it.each([
    ["en", en, ["Tools", "Routing", "Models"]],
    ["zh", zh, ["工具", "路由", "模型"]],
    ["ja", ja, ["ツール", "ルーティング", "モデル"]],
    ["zh-TW", zhTW, ["工具", "路由", "模型"]],
  ] as const)(
    "uses concise resource labels in %s without changing navigation",
    async (language, locale, labels) => {
      const i18n = createInstance();
      await i18n.use(initReactI18next).init({
        lng: language,
        resources: { [language]: { translation: locale } },
        interpolation: { escapeValue: false },
      });
      const onSelect = vi.fn();
      render(
        <I18nextProvider i18n={i18n}>
          <WorkspaceNavigation view="agents" onSelect={onSelect} />
        </I18nextProvider>,
      );
      const destinations = ["agents", "routing", "providers"] as const;
      const brand = screen.getByRole("button", { name: "Vary AI" });
      expect(brand.querySelector(".workspace-brand-accent")).toHaveTextContent(
        "AI",
      );
      labels.forEach((label, index) => {
        const button = screen.getByRole("button", { name: label });
        expect(button).toHaveTextContent(label);
        expect(button).toHaveAttribute("title", label);
        fireEvent.click(button);
        expect(onSelect).toHaveBeenLastCalledWith(destinations[index]);
      });
    },
  );

  it.each([
    ["en", en, "Settings"],
    ["zh", zh, "设置"],
    ["ja", ja, "設定"],
    ["zh-TW", zhTW, "設定"],
  ] as const)(
    "labels the settings destination in %s",
    async (language, locale, label) => {
      const i18n = createInstance();
      await i18n.use(initReactI18next).init({
        lng: language,
        resources: { [language]: { translation: locale } },
        interpolation: { escapeValue: false },
      });
      const onSelect = vi.fn();
      render(
        <I18nextProvider i18n={i18n}>
          <WorkspaceNavigation view="settings" onSelect={onSelect} />
        </I18nextProvider>,
      );
      const settings = screen.getByRole("button", { name: label });
      expect(settings).toHaveTextContent(label);
      expect(settings).toHaveAttribute("title", label);
      expect(settings).toHaveAttribute("aria-current", "page");
      fireEvent.click(settings);
      expect(onSelect).toHaveBeenCalledWith("settings");
    },
  );
});
