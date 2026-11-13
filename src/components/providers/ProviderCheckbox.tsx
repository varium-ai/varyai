import { Checkbox, type CheckboxProps } from "@/components/ui/checkbox";

export function ProviderCheckbox({
  className,
  checked,
  ...props
}: CheckboxProps) {
  return (
    <span className="pv-checkbox">
      <Checkbox
        {...props}
        checked={checked}
        aria-checked={checked === "indeterminate" ? "mixed" : checked}
        className={`pv-checkbox-input ${className ?? ""}`}
      />
      <span className="pv-checkbox-box" aria-hidden="true">
        <svg viewBox="0 0 16 16" fill="none">
          <path
            className="pv-checkbox-tick"
            d="m3.5 8 3 3 6-6"
            pathLength="1"
          />
          <path className="pv-checkbox-mixed" d="M4 8h8" />
        </svg>
      </span>
    </span>
  );
}
