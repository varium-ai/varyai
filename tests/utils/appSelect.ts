import { fireEvent, screen, waitFor } from "@testing-library/react";
import { expect } from "vitest";

export function selectAppOption(trigger: HTMLElement, value: string) {
  fireEvent.click(trigger);
  const option = screen
    .getAllByRole("option")
    .find((item) => item.getAttribute("data-option-value") === value);
  if (!option) throw new Error(`Missing app option: ${value}`);
  fireEvent.click(option);
}
export async function inspectAppOption(
  trigger: HTMLElement,
  name: string | RegExp,
) {
  await waitFor(() => expect(trigger).toBeEnabled());
  fireEvent.click(trigger);
  const option = await screen.findByRole("option", { name });
  fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
  return option;
}
