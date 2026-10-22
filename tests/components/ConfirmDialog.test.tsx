import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { ConfirmDialog } from "@/components/ConfirmDialog";

describe("ConfirmDialog", () => {
  it("treats backdrop clicks as cancellation rather than confirmation", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        isOpen
        title="Confirm"
        message="Message"
        onCancel={onCancel}
        onConfirm={onConfirm}
      />,
    );
    await user.click(
      screen.getByRole("dialog").previousElementSibling as HTMLElement,
    );
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("prevents cancel and confirm actions while pending", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    const onCancel = vi.fn();

    render(
      <ConfirmDialog
        isOpen
        title="Confirm write"
        message="Please wait"
        pending
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    const cancelButton = screen.getByRole("button", { name: "common.cancel" });
    const confirmButton = screen.getByRole("button", {
      name: "common.confirm",
    });
    expect(cancelButton).toBeDisabled();
    expect(confirmButton).toBeDisabled();

    fireEvent.click(cancelButton);
    fireEvent.click(confirmButton);
    await user.click(
      screen.getByRole("dialog").previousElementSibling as HTMLElement,
    );
    expect(onCancel).not.toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
