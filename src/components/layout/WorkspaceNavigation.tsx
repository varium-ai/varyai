import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Blocks,
  GitBranch,
  Layers3,
  ChartNoAxesCombined,
  ShieldCheck,
  Settings2,
} from "lucide-react";
import appIcon from "@/assets/icons/app-icon.png";
export type WorkspaceView =
  | "agents"
  | "routing"
  | "providers"
  | "stats"
  | "security"
  | "settings";
const items = [
  ["agents", Blocks],
  ["routing", GitBranch],
  ["providers", Layers3],
  ["stats", ChartNoAxesCombined],
  ["security", ShieldCheck],
] as const;
export function WorkspaceNavigation({
  view,
  onSelect,
}: {
  view: WorkspaceView;
  onSelect: (view: WorkspaceView) => void;
}) {
  const { t, i18n } = useTranslation();
  const sidebar = useRef<HTMLElement>(null);
  const [selection, setSelection] = useState<{
    top: number;
    left: number;
    width: number;
    height: number;
  } | null>(null);

  useLayoutEffect(() => {
    const root = sidebar.current;
    const active = root?.querySelector<HTMLElement>('[aria-current="page"]');
    if (!root || !active) return;

    function measure() {
      if (!active) return;
      const next = {
        top: active.offsetTop,
        left: active.offsetLeft,
        width: active.offsetWidth,
        height: active.offsetHeight,
      };
      setSelection((previous) =>
        previous &&
        previous.top === next.top &&
        previous.left === next.left &&
        previous.width === next.width &&
        previous.height === next.height
          ? previous
          : next,
      );
    }

    // Follow the accepted view, including guarded navigation and resized labels.
    measure();
    const observer =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(measure);
    observer?.observe(root);
    const nav = root.querySelector("nav");
    if (nav) observer?.observe(nav);
    observer?.observe(active);
    window.addEventListener("resize", measure);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [view, i18n.language]);

  return (
    <aside
      ref={sidebar}
      className="workspace-sidebar"
      aria-label={t("workspaceNav.label")}
    >
      {selection && (
        <span
          className="workspace-nav-selection"
          aria-hidden="true"
          style={{
            left: selection.left,
            width: selection.width,
            height: selection.height,
            transform: `translate3d(0px, ${selection.top}px, 0)`,
          }}
        />
      )}
      <button
        className="workspace-brand"
        aria-label="Vary AI"
        onClick={() => onSelect("agents")}
      >
        <img src={appIcon} alt="" />
        <span>
          Vary <span className="workspace-brand-accent">AI</span>
        </span>
      </button>
      <nav>
        {items.map(([id, Icon]) => (
          <button
            key={id}
            aria-label={t(`workspaceNav.${id}`)}
            aria-current={view === id ? "page" : undefined}
            title={t(`workspaceNav.${id}`)}
            onClick={() => onSelect(id)}
            className="workspace-nav-item"
          >
            <Icon />
            <span>{t(`workspaceNav.${id}`)}</span>
          </button>
        ))}
      </nav>
      <button
        aria-label={t("workspaceNav.settings")}
        aria-current={view === "settings" ? "page" : undefined}
        title={t("workspaceNav.settings")}
        className="workspace-nav-item workspace-settings"
        onClick={() => onSelect("settings")}
      >
        <Settings2 />
        <span>{t("workspaceNav.settings")}</span>
      </button>
    </aside>
  );
}
