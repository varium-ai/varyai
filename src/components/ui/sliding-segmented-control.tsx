import type { CSSProperties, ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export interface SlidingSegmentedOption<Value extends string> {
  value: Value;
  label: ReactNode;
  ariaLabel?: string;
  title?: string;
  disabled?: boolean;
  testId?: string;
}

interface SlidingSegmentedControlProps<Value extends string> {
  value: Value | null;
  options: readonly SlidingSegmentedOption<Value>[];
  onValueChange: (value: Value) => void;
  ariaLabel: string;
  role?: "group" | "tablist";
  itemRole?: "button" | "tab";
  indicatorVariant?: "primary" | "surface";
  className?: string;
  buttonClassName?: string;
  indicatorClassName?: string;
  indicatorTestId?: string;
  indicatorInset?: "padded" | "none";
}

export function SlidingSegmentedControl<Value extends string>({
  value,
  options,
  onValueChange,
  ariaLabel,
  role = "group",
  itemRole = "button",
  indicatorVariant = "primary",
  className,
  buttonClassName,
  indicatorClassName,
  indicatorTestId,
  indicatorInset = "padded",
}: SlidingSegmentedControlProps<Value>) {
  const count = Math.max(options.length, 1);
  const currentIndex = options.findIndex((option) => option.value === value);
  const selectedIndex = Math.max(currentIndex, 0);
  const surface = indicatorVariant === "surface";
  const inset =
    indicatorInset === "padded" ? "var(--segmented-inset, 4px)" : "0px";

  return (
    <div
      role={role}
      aria-label={ariaLabel}
      data-sliding-indicator={surface ? "" : undefined}
      className={cn(
        "relative inline-grid gap-0",
        surface
          ? "vary-segmented"
          : "rounded-md border border-border-default bg-background p-1",
        className,
      )}
      style={{ gridTemplateColumns: `repeat(${count}, minmax(0, 1fr))` }}
    >
      <span
        aria-hidden="true"
        data-testid={indicatorTestId}
        className={cn(
          "pointer-events-none absolute z-0 transition-transform duration-300 ease-out motion-reduce:transition-none",
          surface
            ? "vary-segmented-indicator"
            : "rounded-sm bg-blue-500 dark:bg-blue-600 [:root[data-renderer=application]_&]:bg-primary",
          indicatorClassName,
        )}
        style={
          {
            insetBlock: inset,
            insetInlineStart: inset,
            width: `calc((100% - 2 * ${inset}) / ${count})`,
            transform: `translateX(${selectedIndex * 100}%)`,
            visibility: currentIndex < 0 ? "hidden" : undefined,
          } as CSSProperties
        }
      />
      {options.map((option) => {
        const selected = option.value === value;

        return (
          <Button
            key={option.value}
            type="button"
            size="sm"
            variant="ghost"
            role={itemRole === "tab" ? "tab" : undefined}
            aria-selected={itemRole === "tab" ? selected : undefined}
            aria-pressed={itemRole === "button" ? selected : undefined}
            aria-label={option.ariaLabel}
            title={option.title}
            data-testid={option.testId}
            disabled={option.disabled}
            onClick={() => onValueChange(option.value)}
            className={cn(
              "relative z-10 w-full min-w-0 justify-center whitespace-nowrap",
              !surface &&
                (selected
                  ? "!text-white hover:!text-white dark:!text-white hover:!bg-transparent dark:hover:!bg-transparent"
                  : "text-muted-foreground hover:text-foreground hover:bg-muted"),
              buttonClassName,
            )}
          >
            {option.label}
          </Button>
        );
      })}
    </div>
  );
}
