import { useRef } from "react";

/** Compact reference-style choices with the keyboard behavior of radio buttons. */
export function RoutingSegments<T extends string>({
  label,
  value,
  options,
  disabled = false,
  onChange,
}: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  disabled?: boolean;
  onChange: (value: T) => void;
}) {
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const active = options.some((option) => option.value === value)
    ? value
    : options[0]?.value;
  return (
    <div className="rt-reference-seg" role="radiogroup" aria-label={label}>
      {options.map((option, index) => (
        <button
          key={option.value}
          ref={(node) => {
            buttons.current[index] = node;
          }}
          type="button"
          role="radio"
          disabled={disabled}
          aria-checked={option.value === value}
          tabIndex={option.value === active ? 0 : -1}
          onClick={() => onChange(option.value)}
          onKeyDown={(event) => {
            let next = index;
            if (["ArrowRight", "ArrowDown"].includes(event.key))
              next = (index + 1) % options.length;
            else if (["ArrowLeft", "ArrowUp"].includes(event.key))
              next = (index + options.length - 1) % options.length;
            else if (event.key === "Home") next = 0;
            else if (event.key === "End") next = options.length - 1;
            else return;
            event.preventDefault();
            onChange(options[next].value);
            buttons.current[next]?.focus({ preventScroll: true });
          }}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
