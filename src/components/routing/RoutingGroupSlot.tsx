import { Component, createRef, type ReactNode } from "react";

// Adapted from Magpie's spring/slot motion; see third_party/magpie/LICENSE.
// Response .42 s, damping .8, settled at 570 ms.
const SPRING =
  "linear(0, 0.0203, 0.0723, 0.1448, 0.2292, 0.3188, 0.4086, 0.4951, 0.576, 0.6498, 0.7157, 0.7735, 0.8232, 0.8654, 0.9005, 0.9293, 0.9525, 0.9708, 0.9849, 0.9956, 1.0033, 1.0087, 1.0122, 1.0142, 1.015, 1.0151, 1.0146, 1.0136, 1.0124, 1.0111, 1.0097, 1.0084, 1.0071, 1.0059, 1.0049, 1.0039, 1.0031, 1.0024, 1.0018, 1.0013, 1)";
const FOLD_EASE = "cubic-bezier(.32, .72, 0, 1)";
function springEase() {
  return typeof CSS !== "undefined" &&
    CSS.supports?.("animation-timing-function", "linear(0, 1)")
    ? SPRING
    : "cubic-bezier(.2, .9, .25, 1.02)";
}

interface RoutingGroupSlotProps {
  expanded: boolean;
  editor: ReactNode;
  children?: ReactNode;
  onCollapseComplete?: () => void;
}

interface SlotSnapshot {
  height: number;
  scroll: HTMLElement | null;
  scrollTop: number;
  top: number;
}

export class RoutingGroupSlot extends Component<RoutingGroupSlotProps> {
  private element = createRef<HTMLLIElement>();
  private editor = createRef<HTMLDivElement>();
  private summary = createRef<HTMLDivElement>();
  private animations: Animation[] = [];
  private preference: MediaQueryList | null = null;
  private stopScroll: (() => void) | null = null;
  private finishScroll: (() => void) | null = null;
  private collapsePending = false;

  private stopAnimation = () => {
    for (const animation of this.animations) {
      animation.onfinish = null;
      animation.cancel();
    }
    this.animations = [];
    this.stopScroll?.();
    this.stopScroll = null;
    this.finishScroll = null;
    if (this.element.current) this.element.current.style.overflow = "";
  };

  private complete = () => {
    this.finishScroll?.();
    this.stopAnimation();
    this.collapsed();
  };

  private collapsed() {
    if (!this.props.expanded && this.collapsePending) {
      this.collapsePending = false;
      this.props.onCollapseComplete?.();
    }
  }

  private preferenceChanged = () => {
    if (this.preference?.matches) this.complete();
  };

  componentDidMount() {
    if (typeof window.matchMedia === "function") {
      this.preference = window.matchMedia("(prefers-reduced-motion: reduce)");
      this.preference.addEventListener("change", this.preferenceChanged);
    }
    if (this.props.expanded) {
      this.transitionFrom({ ...this.snapshot(), height: 0 });
    }
  }

  private snapshot(): SlotSnapshot {
    const element = this.element.current;
    const bounds = element?.getBoundingClientRect();
    let scroll = element?.parentElement ?? null;
    while (scroll && !/auto|scroll/.test(getComputedStyle(scroll).overflowY)) {
      scroll = scroll.parentElement;
    }
    return {
      height: bounds?.height ?? 0,
      scroll,
      scrollTop: scroll?.scrollTop ?? 0,
      top:
        (bounds?.top ?? 0) -
        (scroll?.getBoundingClientRect().top ?? 0) -
        (scroll?.clientTop ?? 0),
    };
  }

  getSnapshotBeforeUpdate(previous: RoutingGroupSlotProps) {
    return previous.expanded !== this.props.expanded ? this.snapshot() : null;
  }

  componentDidUpdate(
    _previous: RoutingGroupSlotProps,
    _state: unknown,
    snapshot: SlotSnapshot | null,
  ) {
    if (snapshot) this.transitionFrom(snapshot);
  }

  componentWillUnmount() {
    this.stopAnimation();
    this.preference?.removeEventListener("change", this.preferenceChanged);
  }

  private followScroll(snapshot: SlotSnapshot, height: number) {
    const { scroll, scrollTop, top } = snapshot;
    if (!scroll) return;
    const anchor = scroll.style.overflowAnchor;
    const behavior = scroll.style.scrollBehavior;
    scroll.style.overflowAnchor = "none";
    scroll.style.scrollBehavior = "auto";
    scroll.scrollTop = scrollTop;
    let frame = 0;
    let following = !this.props.expanded && top < 0;
    const target = Math.max(0, scrollTop + top);
    const interrupt = () => {
      following = false;
    };
    const events = ["wheel", "touchstart", "pointerdown", "keydown"] as const;
    for (const event of events)
      scroll.addEventListener(event, interrupt, { passive: true });
    const step = () => {
      if (!following || !this.element.current) return;
      const current = this.element.current.getBoundingClientRect().height;
      const progress = Math.min(
        1,
        Math.max(0, (current - snapshot.height) / (height - snapshot.height)),
      );
      scroll.scrollTop = scrollTop + (target - scrollTop) * progress;
      if (progress < 1) frame = requestAnimationFrame(step);
    };
    if (following) frame = requestAnimationFrame(step);
    this.finishScroll = () => {
      if (following) scroll.scrollTop = target;
    };
    this.stopScroll = () => {
      cancelAnimationFrame(frame);
      for (const event of events) scroll.removeEventListener(event, interrupt);
      scroll.style.overflowAnchor = anchor;
      scroll.style.scrollBehavior = behavior;
    };
  }

  private transitionFrom(snapshot: SlotSnapshot) {
    this.stopAnimation();
    this.collapsePending = !this.props.expanded;
    const element = this.element.current;
    const height = element?.getBoundingClientRect().height ?? 0;
    if (
      !element ||
      typeof element.animate !== "function" ||
      this.preference?.matches ||
      Math.abs(snapshot.height - height) < 1
    ) {
      if (!this.props.expanded && snapshot.scroll && snapshot.top < 0) {
        snapshot.scroll.scrollTop = Math.max(
          0,
          snapshot.scrollTop + snapshot.top,
        );
      }
      this.complete();
      return;
    }
    try {
      element.style.overflow = "hidden";
      const grow = element.animate(
        [{ height: `${snapshot.height}px` }, { height: `${height}px` }],
        {
          duration: this.props.expanded ? 570 : 420,
          easing: this.props.expanded ? springEase() : FOLD_EASE,
        },
      );
      this.animations.push(grow);
      grow.onfinish = () => {
        this.finishScroll?.();
        this.stopScroll?.();
        this.stopScroll = null;
        this.finishScroll = null;
        element.style.overflow = "";
        this.animations = this.animations.filter(
          (animation) => animation !== grow,
        );
        grow.onfinish = null;
        grow.cancel();
        this.collapsed();
      };
      this.followScroll(snapshot, height);
      if (this.props.expanded) {
        const form = this.editor.current?.firstElementChild;
        const sections = [...(form?.children ?? [])].flatMap((section) =>
          section.tagName === "FIELDSET" ? [...section.children] : [section],
        );
        for (const [index, section] of sections.entries()) {
          this.animations.push(
            section.animate(
              [
                { opacity: 0, transform: "translateY(-6px)" },
                { opacity: 1, transform: "none" },
              ],
              {
                duration: 420,
                delay: 40 + Math.min(index, 12) * 18,
                easing: FOLD_EASE,
                fill: "backwards",
              },
            ),
          );
        }
      } else if (this.summary.current) {
        this.animations.push(
          this.summary.current.animate([{ opacity: 0 }, { opacity: 1 }], {
            duration: 300,
            delay: 90,
            easing: "ease-out",
            fill: "backwards",
          }),
        );
      }
    } catch {
      this.complete();
    }
  }

  render() {
    return (
      <li
        ref={this.element}
        className="relative min-w-0"
        style={{ overflowAnchor: "none" }}
      >
        {this.props.expanded ? (
          <div ref={this.editor}>{this.props.editor}</div>
        ) : (
          <div ref={this.summary}>{this.props.children}</div>
        )}
      </li>
    );
  }
}
