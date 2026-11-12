import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { ConfirmDialog } from "@/components/ConfirmDialog";

describe("ConfirmDialog", () => {
  it("resets optional consent on reopen and never submits an absent checkbox default", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    const props = {
      title: "Delete provider",
      message: "Remove this provider?",
      onCancel: vi.fn(),
      onConfirm,
    };
    const { rerender } = render(
      <ConfirmDialog
        {...props}
        isOpen
        checkboxLabel="Remove preferences"
        checkboxDefaultChecked
      />,
    );
    const checkbox = screen.getByRole("checkbox", {
      name: "Remove preferences",
    });
    expect(checkbox).toBeChecked();
    await user.click(checkbox);
    await user.click(screen.getByRole("button", { name: "common.confirm" }));
    expect(onConfirm).toHaveBeenLastCalledWith(false);
    rerender(
      <ConfirmDialog
        {...props}
        isOpen={false}
        checkboxLabel="Remove preferences"
        checkboxDefaultChecked
      />,
    );
    rerender(
      <ConfirmDialog
        {...props}
        isOpen
        checkboxLabel="Remove preferences"
        checkboxDefaultChecked
      />,
    );
    expect(screen.getByRole("checkbox")).toBeChecked();
    await user.click(screen.getByRole("button", { name: "common.confirm" }));
    expect(onConfirm).toHaveBeenLastCalledWith(true);
    rerender(<ConfirmDialog {...props} isOpen checkboxDefaultChecked />);
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "common.confirm" }));
    expect(onConfirm).toHaveBeenLastCalledWith(false);
  });

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
    expect(confirmButton).toHaveAttribute("aria-busy", "true");
    expect(confirmButton.querySelector("svg.animate-spin")).toBeInTheDocument();

    fireEvent.click(cancelButton);
    fireEvent.click(confirmButton);
    await user.click(
      screen.getByRole("dialog").previousElementSibling as HTMLElement,
    );
    expect(onCancel).not.toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
