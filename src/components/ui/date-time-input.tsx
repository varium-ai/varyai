import * as React from "react";
import { Input } from "./input";

interface DateTimeInputProps
  extends Omit<React.ComponentProps<typeof Input>, "type"> {
  type: "date" | "time";
  value: string;
}
function valid(value: string, type: "date" | "time"): boolean {
  if (type === "time") return /^(?:[01]\d|2[0-3]):[0-5]\d$/.test(value);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const [year, month, day] = value.split("-").map(Number);
  if (year < 1 || month < 1 || month > 12 || day < 1) return false;
  const date = new Date(0);
  date.setFullYear(year, month - 1, day);
  return (
    date.getFullYear() === year &&
    date.getMonth() === month - 1 &&
    date.getDate() === day
  );
}

/** Keeps partial edits local; only valid values reach the existing range handlers. */
export function DateTimeInput({
  type,
  value,
  onChange,
  onBlur,
  ...props
}: DateTimeInputProps) {
  const [draft, setDraft] = React.useState(value);
  React.useEffect(() => setDraft(value), [value]);
  return (
    <Input
      {...props}
      type="text"
      inputMode="numeric"
      autoComplete="off"
      spellCheck={false}
      value={draft}
      onChange={(event) => {
        setDraft(event.target.value);
        if (valid(event.target.value, type)) onChange?.(event);
      }}
      onBlur={(event) => {
        if (!valid(draft, type)) setDraft(value);
        onBlur?.(event);
      }}
    />
  );
}
