import { useTranslation } from "react-i18next";
import { useLayoutEffect, useRef, type ReactNode } from "react";
import appIcon from "@/assets/icons/app-icon.png";
import { APP_DISPLAY_NAME } from "@/config/brand";

export type PrimaryTab = "home" | "security" | "stats" | "settings";

interface PrimaryNavigationProps {
  activeTab: PrimaryTab | null;
  onSelect: (tab: PrimaryTab) => void;
  actions?: ReactNode;
  disabled?: boolean;
}

const tabs: PrimaryTab[] = ["home", "security", "stats", "settings"];
const [brandName, brandAccent] = APP_DISPLAY_NAME.split(/ (?=AI$)/);

export function PrimaryNavigation({
  activeTab,
  onSelect,
  actions,
  disabled = false,
}: PrimaryNavigationProps) {
  const { t } = useTranslation();
  const trackRef = useRef<HTMLDivElement>(null);
  const indicatorRef = useRef<HTMLSpanElement>(null);
  const underlineRef = useRef<HTMLSpanElement>(null);
  const tabRefs = useRef<Partial<Record<PrimaryTab, HTMLButtonElement | null>>>(
    {},
  );
  const previousTabRef = useRef<PrimaryTab | null>(null);
  const alignRef = useRef<() => void>(() => {});
  const settleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const finishTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isAnimatingRef = useRef(false);

  const clearAnimationTimers = () => {
    if (settleTimerRef.current) clearTimeout(settleTimerRef.current);
    if (finishTimerRef.current) clearTimeout(finishTimerRef.current);
    settleTimerRef.current = null;
    finishTimerRef.current = null;
  };

  useLayoutEffect(() => {
    const track = trackRef.current;
    const indicator = indicatorRef.current;
    const underline = underlineRef.current;
    if (!track || !indicator || !underline) return;

    const previousTab = previousTabRef.current;
    previousTabRef.current = activeTab;
    const motionPreference = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)",
    );

    const alignIndicator = () => {
      const button = activeTab ? tabRefs.current[activeTab] : null;
      if (!button) {
        clearAnimationTimers();
        isAnimatingRef.current = false;
        indicator.dataset.phase = "idle";
        underline.dataset.phase = "idle";
        indicator.style.visibility = "hidden";
        underline.style.visibility = "hidden";
        return;
      }

      const trackRect = track.getBoundingClientRect();
      const targetRect = button.getBoundingClientRect();
      // Read the painted box before interrupting a transition so rapid clicks
      // continue from the position currently visible on screen.
      const currentRect = indicator.getBoundingClientRect();
      const currentUnderlineRect = underline.getBoundingClientRect();
      const fromLeft = currentRect.left - trackRect.left;
      const fromWidth = currentRect.width;
      const fromUnderlineLeft = currentUnderlineRect.left - trackRect.left;
      const toLeft = targetRect.left - trackRect.left;
      const toWidth = targetRect.width;
      const toUnderlineLeft = toLeft + (toWidth - 32) / 2;
      const wasVisible = indicator.style.visibility !== "hidden";

      clearAnimationTimers();

      const setIndicatorBox = (left: number, width: number) => {
        indicator.style.left = `${left}px`;
        indicator.style.width = `${width}px`;
      };
      const setIndicatorPhase = (phase: "idle" | "stretch" | "settle") => {
        indicator.dataset.phase = phase;
      };
      const setUnderlinePosition = (left: number) => {
        underline.style.left = `${left}px`;
        underline.style.width = "32px";
      };
      const canAnimate =
        wasVisible &&
        !!previousTab &&
        previousTab !== activeTab &&
        !motionPreference?.matches &&
        fromWidth > 0 &&
        toWidth > 0;

      indicator.style.visibility = toWidth > 0 ? "visible" : "hidden";
      underline.style.visibility = toWidth > 32 ? "visible" : "hidden";
      if (!canAnimate) {
        isAnimatingRef.current = false;
        setIndicatorPhase("idle");
        underline.dataset.phase = "idle";
        setIndicatorBox(toLeft, toWidth);
        setUnderlinePosition(toUnderlineLeft);
        return;
      }

      // Use two ordinary CSS transitions for compatibility with the WKWebView
      // bundled by Tauri. Phase one stretches across both tabs; phase two lets
      // the trailing edge catch up and settle on the destination.
      const stretchLeft = Math.min(fromLeft, toLeft);
      const stretchRight = Math.max(fromLeft + fromWidth, toLeft + toWidth);
      isAnimatingRef.current = true;
      setIndicatorPhase("idle");
      underline.dataset.phase = "idle";
      setIndicatorBox(fromLeft, fromWidth);
      setUnderlinePosition(fromUnderlineLeft);
      // Commit the interrupted position before enabling the next transition.
      void indicator.offsetWidth;
      void underline.offsetWidth;
      setIndicatorPhase("stretch");
      underline.dataset.phase = "slide";
      setIndicatorBox(stretchLeft, stretchRight - stretchLeft);
      setUnderlinePosition(toUnderlineLeft);

      settleTimerRef.current = setTimeout(() => {
        setIndicatorPhase("settle");
        setIndicatorBox(toLeft, toWidth);
      }, 170);
      finishTimerRef.current = setTimeout(() => {
        isAnimatingRef.current = false;
        setIndicatorPhase("idle");
        underline.dataset.phase = "idle";
        setIndicatorBox(toLeft, toWidth);
        setUnderlinePosition(toUnderlineLeft);
      }, 450);
    };

    alignRef.current = alignIndicator;
    alignIndicator();
  }, [activeTab]);

  useLayoutEffect(() => {
    const track = trackRef.current;
    if (!track) return;
    const motionPreference = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)",
    );
    const alignWithoutMotion = () => {
      if (isAnimatingRef.current && !motionPreference?.matches) return;
      clearAnimationTimers();
      isAnimatingRef.current = false;
      alignRef.current();
    };
    // Observe each tab as well as the track: translations can redistribute
    // their widths without changing the total navigation width. Keep the
    // observer mounted so its initial notification cannot cancel a tab switch.
    const observer = new ResizeObserver(alignWithoutMotion);
    observer.observe(track);
    for (const button of Object.values(tabRefs.current)) {
      if (button) observer.observe(button);
    }
    motionPreference?.addEventListener("change", alignWithoutMotion);
    return () => {
      observer.disconnect();
      motionPreference?.removeEventListener("change", alignWithoutMotion);
      clearAnimationTimers();
    };
  }, []);

  return (
    <div className="varyai-primary-nav flex h-16 items-center justify-between gap-5 border-b border-slate-200/90 bg-white px-6 dark:border-slate-700 dark:bg-card">
      <button
        type="button"
        onClick={() => onSelect("home")}
        disabled={disabled}
        className="flex shrink-0 items-center gap-3 text-left disabled:cursor-not-allowed disabled:opacity-50"
        aria-label={t("app.title")}
      >
        <img src={appIcon} alt="" className="h-9 w-9" />
        <span className="flex flex-col leading-none">
          <span className="text-lg font-black tracking-tight text-slate-900 dark:text-foreground">
            {brandName} <span className="text-primary">{brandAccent}</span>
          </span>
          <span className="mt-1 hidden whitespace-nowrap text-[10px] font-medium text-slate-400 lg:block">
            Questions pass, Secrets stay
          </span>
        </span>
      </button>
      <nav
        className="-m-1 min-w-0 overflow-x-auto p-1"
        aria-label={t("primaryNav.label")}
      >
        <div
          ref={trackRef}
          className="relative isolate flex w-max items-center gap-1"
        >
          <span
            ref={indicatorRef}
            data-testid="primary-nav-indicator"
            data-phase="idle"
            aria-hidden="true"
            className={
              "varyai-primary-nav-indicator pointer-events-none absolute inset-y-0 left-0 rounded-full bg-blue-50 dark:bg-blue-500/15" +
              (disabled ? " opacity-50" : "")
            }
            style={{ visibility: "hidden" }}
          />
          <span
            ref={underlineRef}
            data-testid="primary-nav-underline"
            data-phase="idle"
            aria-hidden="true"
            className="varyai-primary-nav-underline pointer-events-none absolute bottom-1 left-0 z-10 h-0.5 rounded-full bg-blue-600 dark:bg-blue-400"
            style={{ visibility: "hidden" }}
          />
          {tabs.map((tab) => (
            <button
              key={tab}
              ref={(element) => {
                tabRefs.current[tab] = element;
              }}
              type="button"
              onClick={() => onSelect(tab)}
              disabled={disabled}
              aria-current={activeTab === tab ? "page" : undefined}
              className={
                "relative shrink-0 rounded-full px-4 py-2 text-xs font-medium transition-colors motion-reduce:transition-none disabled:cursor-not-allowed disabled:opacity-50 " +
                (activeTab === tab
                  ? "text-blue-600 dark:text-blue-400"
                  : "text-slate-600 hover:text-slate-900 dark:text-muted-foreground dark:hover:text-foreground")
              }
            >
              {t(`primaryNav.${tab}`)}
            </button>
          ))}
        </div>
      </nav>
      {actions && (
        <div className="flex shrink-0 items-center gap-3">{actions}</div>
      )}
    </div>
  );
}
