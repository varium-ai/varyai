import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { WorkspacePageHeading } from "@/components/layout/WorkspacePageHeading";

it("renders one page title and only reserves description space when needed", () => {
  const { rerender } = render(<WorkspacePageHeading title="Tools" />);
  expect(
    screen.getByRole("heading", { level: 1, name: "Tools" }).nextElementSibling,
  ).toBeNull();
  rerender(
    <WorkspacePageHeading title="Security" description="Audit records" />,
  );
  expect(
    screen.getByRole("heading", { level: 1, name: "Security" })
      .nextElementSibling,
  ).toBe(screen.getByText("Audit records"));
});
