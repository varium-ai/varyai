import type { ButtonHTMLAttributes } from "react";
import { Check } from "lucide-react";

export function SubscriptionModelChip({
  selected,
  children,
  ...props
}: Omit<ButtonHTMLAttributes<HTMLButtonElement>, "className"> & {
  selected: boolean;
}) {
  return (
    <button
      {...props}
      type="button"
      className={`pv-model-chip ${selected ? "selected on" : ""}`}
      aria-pressed={selected}
    >
      <Check
        size={11}
        aria-hidden
        style={{ visibility: selected ? "visible" : "hidden" }}
      />
      <span>{children}</span>
    </button>
  );
}
