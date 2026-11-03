import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useDialogSnapshot } from "@/hooks/useDialogSnapshot";

describe("dialog snapshots", () => {
  it("retains the last content until exit finishes and then releases it", () => {
    const value = { name: "Synthetic confirmation" };
    const { result, rerender } = renderHook(({ value }) => useDialogSnapshot(value), {
      initialProps: { value: value as typeof value | null },
    });
    rerender({ value: null });
    expect(result.current.snapshot).toBe(value);
    act(() => result.current.onExited());
    expect(result.current.snapshot).toBeNull();
  });

  it("keeps newly opened content when an earlier exit callback arrives", () => {
    const { result, rerender } = renderHook(({ value }) => useDialogSnapshot(value), {
      initialProps: { value: "first" as string | null },
    });
    rerender({ value: null });
    const previousExit = result.current.onExited;
    rerender({ value: "second" });
    act(previousExit);
    expect(result.current.snapshot).toBe("second");
    rerender({ value: null });
    expect(result.current.snapshot).toBe("second");
  });
});
