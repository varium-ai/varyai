import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentProps,
  type CSSProperties,
} from "react";
import { PopoverContent } from "@/components/ui/popover";
import { modelPickerPlacement } from "./modelPickerPlacement";
import "./model-picker.css";

export function ModelPickerPopoverContent({
  onDismiss,
  children,
  ...props
}: Omit<ComponentProps<typeof PopoverContent>, "className" | "ref"> & {
  onDismiss: () => void;
}) {
  const [panel, setPanel] = useState<HTMLDivElement | null>(null);
  const [placement, setPlacement] = useState<ReturnType<
    typeof modelPickerPlacement
  > | null>(null);
  const dismiss = useRef(onDismiss);
  dismiss.current = onDismiss;
  useLayoutEffect(() => {
    if (!panel) return;
    // Radix links its trigger and content through aria-controls.
    const trigger = Array.from(
      document.querySelectorAll<HTMLElement>("[aria-controls]"),
    ).find((element) => element.getAttribute("aria-controls") === panel.id);
    if (!trigger) return;
    const list = panel.querySelector<HTMLElement>(".mp-am-list");
    const naturalHeight = Math.max(
      panel.scrollHeight,
      panel.offsetHeight + (list ? list.scrollHeight - list.clientHeight : 0),
    );
    setPlacement(
      modelPickerPlacement({
        trigger: trigger.getBoundingClientRect(),
        viewport: { width: window.innerWidth, height: window.innerHeight },
        width: panel.getBoundingClientRect().width,
        height: naturalHeight,
        maxHeight: panel.dataset.variant === "agent" ? 420 : 560,
      }),
    );
  }, [panel]);
  useEffect(() => {
    if (!panel) return;
    const scrolled = (event: Event) => {
      if (event.target instanceof Node && panel.contains(event.target)) return;
      dismiss.current();
    };
    const resized = () => dismiss.current();
    document.addEventListener("scroll", scrolled, true);
    window.addEventListener("resize", resized);
    return () => {
      document.removeEventListener("scroll", scrolled, true);
      window.removeEventListener("resize", resized);
    };
  }, [panel]);
  return (
    <PopoverContent
      ref={setPanel}
      className="mp-model-popover mp-model-picker-popover"
      data-mac={/Mac/.test(navigator.platform)}
      align="start"
      alignOffset={-10}
      sideOffset={8}
      collisionPadding={8}
      side={placement?.side ?? "bottom"}
      style={
        {
          maxHeight: placement?.maxHeight,
          "--mp-model-origin-x": placement
            ? `${placement.originX}px`
            : undefined,
        } as CSSProperties
      }
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        if (event.target instanceof HTMLElement)
          event.target
            .querySelector<HTMLInputElement>('input[type="search"]')
            ?.focus();
      }}
      {...props}
    >
      {children}
    </PopoverContent>
  );
}
