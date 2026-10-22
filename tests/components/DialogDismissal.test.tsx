import { useState } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";

function Dialogs() {
  const [open, setOpen] = useState(false);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <button>Open dialog</button>
      </DialogTrigger>
      <DialogContent>
        <DialogTitle>Parent dialog</DialogTitle>
        <DialogDescription>Parent content</DialogDescription>
        <button>Inside action</button>
        <Dialog>
          <DialogTrigger asChild>
            <button>Open nested dialog</button>
          </DialogTrigger>
          <DialogContent zIndex="nested">
            <DialogTitle>Nested dialog</DialogTitle>
            <DialogDescription>Nested content</DialogDescription>
          </DialogContent>
        </Dialog>
      </DialogContent>
    </Dialog>
  );
}

describe("Dialog backdrop dismissal", () => {
  it("closes on backdrop click, preserves content clicks and restores focus", async () => {
    const user = userEvent.setup();
    render(<Dialogs />);
    const trigger = screen.getByRole("button", {
      name: "Open dialog",
    });
    await user.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Parent dialog" });
    await user.click(
      within(dialog).getByRole("button", { name: "Inside action" }),
    );
    expect(dialog).toBeVisible();
    await user.click(dialog.previousElementSibling as HTMLElement);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(trigger).toHaveFocus();
  });

  it("dismisses only the topmost dialog when its backdrop is clicked", async () => {
    const user = userEvent.setup();
    render(<Dialogs />);
    await user.click(screen.getByRole("button", { name: "Open dialog" }));
    const parent = screen.getByRole("dialog", { name: "Parent dialog" });
    const nestedTrigger = within(parent).getByRole("button", {
      name: "Open nested dialog",
    });
    await user.click(nestedTrigger);
    const child = screen.getByRole("dialog", { name: "Nested dialog" });
    await user.click(child.previousElementSibling as HTMLElement);
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Nested dialog" }),
      ).toBeNull(),
    );
    expect(screen.getByRole("dialog", { name: "Parent dialog" })).toBeVisible();
    expect(nestedTrigger).toHaveFocus();
    await user.click(parent.previousElementSibling as HTMLElement);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
