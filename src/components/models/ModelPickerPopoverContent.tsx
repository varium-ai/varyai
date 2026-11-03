import { useEffect, useRef, useState, type ComponentProps } from "react";
import { Arrow as PopoverArrow } from "@radix-ui/react-popover";
import { PopoverContent } from "@/components/ui/popover";
import "./model-picker.css";

export function ModelPickerPopoverContent({
  onDismiss,
  children,
  ...props
}: Omit<ComponentProps<typeof PopoverContent>, "className" | "ref"> & {
  onDismiss: () => void;
}) {
  const [panel, setPanel] = useState<HTMLDivElement | null>(null);
  const dismiss = useRef(onDismiss);
  dismiss.current = onDismiss;
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
      align="start"
      alignOffset={-10}
      sideOffset={8}
      collisionPadding={8}
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        if (event.target instanceof HTMLElement)
          event.target
            .querySelector<HTMLInputElement>('input[type="search"]')
            ?.focus();
      }}
      {...props}
    >
      <PopoverArrow className="mp-am-arrow" width={12} height={6} />
      {children}
    </PopoverContent>
  );
}
