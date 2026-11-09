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
      {selected && <Check size={12} aria-hidden />}
      <span>{children}</span>
    </button>
  );
}
