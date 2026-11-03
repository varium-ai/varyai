import { useCallback, useRef, useState } from "react";

// Nullable dialog data may clear immediately; retain its view through CSS exit.
export function useDialogSnapshot<T>(value: T | null) {
  const [snapshot, setSnapshot] = useState(value);
  const current = useRef(value);
  current.current = value;
  if (value !== null && value !== snapshot) setSnapshot(value);
  const onExited = useCallback(() => {
    if (current.current === null) setSnapshot(null);
  }, []);
  return { snapshot: value ?? snapshot, onExited };
}
