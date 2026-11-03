import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { ProviderIcon } from "@/components/ProviderIcon";

it.each(["zai", "groq"])(
  "renders %s as a current-color inline logo for both themes",
  (icon) => {
    const { container } = render(
      <ProviderIcon icon={icon} name="Synthetic vendor" size={18} />,
    );
    expect(container.querySelector('svg[fill="currentColor"]')).not.toBeNull();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  },
);
