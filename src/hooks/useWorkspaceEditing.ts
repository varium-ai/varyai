import { useEffect } from "react";

export type EditingState = { dirty: boolean; busy: boolean };
export function useWorkspaceEditing(
  dirty: boolean,
  busy: boolean,
  onChange?: (state: EditingState) => void,
) {
  useEffect(() => {
    onChange?.({ dirty, busy });
    return () => onChange?.({ dirty: false, busy: false });
  }, [dirty, busy, onChange]);
  useEffect(() => {
    if (!dirty && !busy) return;
    function beforeUnload(event: BeforeUnloadEvent) {
      event.preventDefault();
      event.returnValue = "";
    }
    window.addEventListener("beforeunload", beforeUnload);
    return () => window.removeEventListener("beforeunload", beforeUnload);
  }, [dirty, busy]);
}
