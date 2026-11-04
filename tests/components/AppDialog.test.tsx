import { useState } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AppDialog } from "@/components/ui/app-dialog";
import { Button } from "@/components/ui/button";
import {
  finishDialogAnimation,
  mockDialogAnimationStyles,
} from "../utils/dialogAnimation";

describe("AppDialog", () => {
  it("names and describes the dialog, focuses the surface without scrolling and dismisses with Escape", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    render(
      <AppDialog
        open
        onOpenChange={onOpenChange}
        title="Confirm update"
        description="More than one installation was found."
        footer={<Button>Continue update</Button>}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Confirm update" });
    expect(dialog).toHaveAccessibleDescription(
      "More than one installation was found.",
    );
    expect(dialog).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("blocks backdrop and Escape dismissal while an action is pending", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    const { rerender } = render(
      <AppDialog open pending title="Saving" onOpenChange={onOpenChange} />,
    );
    await user.keyboard("{Escape}");
    await user.click(
      screen.getByRole("dialog").previousElementSibling as HTMLElement,
    );
    expect(onOpenChange).not.toHaveBeenCalled();
    rerender(<AppDialog open title="Saving" onOpenChange={onOpenChange} />);
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("retains nullable content through its exit and returns focus to the opener", async () => {
    mockDialogAnimationStyles();
    const user = userEvent.setup();
    function Fixture() {
      const [target, setTarget] = useState<string | null>(null);
      return (
        <>
          <button onClick={() => setTarget("Synthetic tool")}>
            Open update
          </button>
          <AppDialog
            open={target !== null}
            title={target ? `Update ${target}` : ""}
            description={target ? `${target} has multiple installations.` : ""}
            onOpenChange={(open) => {
              if (!open) setTarget(null);
            }}
            footer={
              <Button onClick={() => setTarget(null)}>Cancel update</Button>
            }
          />
        </>
      );
    }
    render(<Fixture />);
    const opener = screen.getByRole("button", { name: "Open update" });
    await user.click(opener);
    const dialog = screen.getByRole("dialog", {
      name: "Update Synthetic tool",
    });
    await user.click(screen.getByRole("button", { name: "Cancel update" }));
    expect(dialog).toHaveAttribute("data-state", "closed");
    expect(dialog).toHaveTextContent(
      "Synthetic tool has multiple installations.",
    );
    finishDialogAnimation(dialog);
    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it("preserves custom dismissal guards", () => {
    const onOpenChange = vi.fn();
    render(
      <AppDialog
        open
        title="Unsaved changes"
        onOpenChange={onOpenChange}
        onEscapeKeyDown={(event) => event.preventDefault()}
      />,
    );
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onOpenChange).not.toHaveBeenCalled();
  });
});
