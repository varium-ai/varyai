import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  finishDialogAnimation,
  mockDialogAnimationStyles,
} from "../utils/dialogAnimation";

function Fixture({
  open,
  fullscreen = false,
}: {
  open: boolean;
  fullscreen?: boolean;
}) {
  return (
    <Dialog open={open}>
      <DialogTrigger>Open</DialogTrigger>
      <DialogContent variant={fullscreen ? "fullscreen" : "default"}>
        <DialogTitle>Animated dialog</DialogTitle>
        <DialogDescription>
          Content stays until exit completes
        </DialogDescription>
      </DialogContent>
    </Dialog>
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("shared dialog motion", () => {
  it.each([false, true])(
    "animates the surface and backdrop (fullscreen: %s)",
    (fullscreen) => {
      render(<Fixture open fullscreen={fullscreen} />);
      const dialog = screen.getByRole("dialog");
      expect(dialog).toHaveClass("vary-dialog-motion");
      expect(dialog.previousElementSibling).toHaveClass(
        "vary-dialog-backdrop-motion",
      );
      expect(dialog).not.toHaveClass("data-[state=open]:animate-in");
    },
  );

  it("retains the surface through exit, ignores child animation events and restores focus", async () => {
    mockDialogAnimationStyles();
    const { rerender } = render(<Fixture open />);
    const dialog = screen.getByRole("dialog");
    rerender(<Fixture open={false} />);
    expect(dialog).toBeInTheDocument();
    expect(dialog).toHaveAttribute("data-state", "closed");
    finishDialogAnimation(
      screen.getByText("Content stays until exit completes"),
    );
    expect(dialog).toBeInTheDocument();
    finishDialogAnimation(dialog);
    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Open" })).toHaveFocus(),
    );
  });

  it("does not remove a dialog reopened before its exit ends", () => {
    mockDialogAnimationStyles();
    const { rerender } = render(<Fixture open />);
    const dialog = screen.getByRole("dialog");
    rerender(<Fixture open={false} />);
    rerender(<Fixture open />);
    finishDialogAnimation(dialog);
    expect(dialog).toHaveAttribute("data-state", "open");
    expect(dialog).toBeInTheDocument();
  });

  it("unmounts immediately when CSS disables animation", () => {
    const { rerender } = render(<Fixture open />);
    rerender(<Fixture open={false} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
